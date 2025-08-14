#![allow(dead_code)]

use std::{
    collections::HashMap,
    fs,
    io::Write,
    num::ParseIntError,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, bail};
use clap::{
    CommandFactory, FromArgMatches, Parser, Subcommand,
    builder::{TypedValueParser, ValueParserFactory},
};
use itertools::Itertools;
use log::{error, info, trace, warn};
use owo_colors::OwoColorize;
use rustsat::solvers::InterruptSolver;
use thiserror::Error;
use timberborn_support_solver::{
    PlatformLimits, Project, Solution, SolverConfig, SolverResponse, SolverRunConfig,
    ValidationResult,
    dimensions::Dimensions,
    grid::Grid,
    platform::{PLATFORMS_DEFAULT, PlatformDef},
    world::World,
};
use tokio::select;
use tokio_util::{future::FutureExt, sync::CancellationToken};

#[derive(Debug, Parser)]
#[command(multicall = true, arg_required_else_help = true, subcommand_required = true)]
#[command(help_template = r#"
{before-help}
{all-args}{after-help}
"#)]
struct ReplCli {
    #[command(subcommand)]
    cmd: ReplCommand,
}

#[derive(Debug, Subcommand)]
enum ReplCommand {
    /// Load a project
    #[command(visible_aliases = ["l"])]
    Load { path: PathBuf },
    /// Reload the current project from the most recent path
    #[command(visible_aliases = ["r", "rel"])]
    Reload,
    /// View the currently loaded project's terrain
    #[command(visible_aliases = ["view"])]
    Terrain,
    /// Solve platform placement for the currently loaded project with optional
    /// solver limits.
    #[command(visible_aliases = ["s"])]
    Solve {
        /// Limits for platform types
        ///
        /// Limits are specified as `k:v` key-value pairs.
        /// Multiple limits may be separated by commas, or the flag may be
        /// specified multiple times.
        ///
        /// Example: `-l5:2,3:4 -l1:10`
        /// ~ At most 2 5x5 platforms, at most 4 3x3 or larger, at most 10 1x1
        /// or larger.
        #[arg(short = 'l', value_delimiter = ',')]
        limits: Vec<PlatformLimitArg>,
    },
    #[command(visible_aliases = ["q"])]
    Exit,
    #[command(name = "?")]
    ShowHelp,
}

#[derive(Error, Debug)]
enum PlatformLimitError {
    #[error("duplicate limit for `{0}`")]
    Duplicate(PlatformDef),
    #[error("no platform with dimensions `{w}x{h}` found", w = .0.width, h = .0.height)]
    Unknown(Dimensions),
}

fn try_into_platform_limits(
    limit_args: Vec<PlatformLimitArg>,
    dims_platform_map: &HashMap<Dimensions, PlatformDef>,
) -> Result<PlatformLimits, PlatformLimitError> {
    let mut map = HashMap::new();
    for PlatformLimitArg(dims, count) in limit_args.into_iter() {
        let def = *dims_platform_map.get(&dims).ok_or(PlatformLimitError::Unknown(dims))?;

        if map.insert(def, count).is_some() {
            // Duplicate value
            // Maybe needs a better error type? it's just one error path for now though
            return Err(PlatformLimitError::Duplicate(def));
        }
    }

    Ok(PlatformLimits::new(map))
}

/// Note: describes limits for _dimensions_, not the platform defs themselves.
/// Platform defs are uniquely identified by their dimensions in the solver, and
/// the CLI accepts dimensions rather than particular platform defs, so this
/// mapping has to be resolved after parsing.
#[derive(Clone, Debug)]
struct PlatformLimitArg(Dimensions, usize);

#[derive(Error, Debug)]
enum PlatformLimitArgParseError {
    #[error("missing/invalid delimiter")]
    MissingDelimiter,
    #[error("invalid platform type `{0}`")]
    InvalidType(String),
    #[error("expected non-negative integer")]
    InvalidValue(ParseIntError),
}

impl FromStr for PlatformLimitArg {
    type Err = PlatformLimitArgParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use PlatformLimitArgParseError as Error;
        let (key, value) = s.split_once(':').ok_or(Error::MissingDelimiter)?;
        let dims = if let Some((a, b)) = key.split_once('x') {
            // Try (a)x(b)
            Dimensions::new(
                a.parse().map_err(Error::InvalidValue)?,
                b.parse().map_err(Error::InvalidValue)?,
            )
        } else {
            // Assume (a)x(a)
            let size: usize = key.parse().map_err(Error::InvalidValue)?;
            Dimensions::new(size, size)
        };

        let count: usize = value.trim().parse().map_err(Error::InvalidValue)?;

        Ok(PlatformLimitArg(dims, count))
    }
}

#[derive(Debug, Error)]
enum ReplParseError {
    #[error("Input was empty")]
    Empty,
    #[error(transparent)]
    ParseError(#[from] anyhow::Error),
    #[error(transparent)]
    Clap(#[from] clap::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn parse_repl() -> Result<ReplCli, ReplParseError> {
    write!(std::io::stdout(), "$ ")?;
    std::io::stdout().flush()?;
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer).context("could not read stdin")?;
    if buffer.trim().is_empty() {
        return Err(ReplParseError::Empty);
    }

    let args = shlex::split(buffer.trim()).context("invalid quoting")?;
    Ok(ReplCli::try_parse_from(&args)?)
}

async fn repl_loop() -> anyhow::Result<()> {
    ReplCli::command().print_long_help()?;

    struct State {
        loaded_project: Option<LoadedProject>,
        dims_platform_map: HashMap<Dimensions, PlatformDef>,
    }

    struct LoadedProject {
        project: Project,
        path: PathBuf,
    }

    let dims_platform_map: HashMap<Dimensions, PlatformDef> = {
        timberborn_support_solver::encoder::dims_platform_map(&PLATFORMS_DEFAULT)
            .into_iter()
            .map(|(k, v)| (k, *v.iter().next().unwrap()))
            .collect()
    };

    let mut state = State { loaded_project: None, dims_platform_map };

    loop {
        if let Some(LoadedProject { project: _project, path }) = &state.loaded_project {
            // Try to show just the file name, fall back to the whole path
            let proj_path_str = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
            println!("Currently loaded project: {proj_path_str}");
        };

        let cli = match parse_repl() {
            Ok(cli) => cli,
            Err(ReplParseError::Empty) => {
                ReplCli::command().print_long_help()?;
                continue;
            }
            Err(err) => {
                print!("{}", err);
                continue;
            }
        };
        let cmd = cli.cmd;

        if matches!(cmd, ReplCommand::Exit) {
            return Ok(());
        }

        if let Err(err) = run_cmd(cmd, &mut state).await {
            error!("{}", err);
        }
    }

    async fn run_cmd(cmd: ReplCommand, state: &mut State) -> anyhow::Result<()> {
        match cmd {
            ReplCommand::Load { path } => {
                state.loaded_project = Some(LoadedProject { project: load_project(&path)?, path });
                println!("Loaded");

                Ok(())
            }
            ReplCommand::Reload => {
                let Some(LoadedProject { path, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                state.loaded_project =
                    Some(LoadedProject { project: load_project(path)?, path: path.clone() });
                println!("Loaded");

                Ok(())
            }
            ReplCommand::Terrain => {
                let Some(LoadedProject { project, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                print_world(&project.world, None, &Default::default());

                Ok(())
            }
            ReplCommand::Solve { limits } => {
                let limits = try_into_platform_limits(limits, &state.dims_platform_map)?;
                let Some(LoadedProject { project, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                let solver = SolverConfig::new(&project.world);
                let run_config = SolverRunConfig { limits };

                let res = select! {
                    res = run_solver(project, solver, run_config) => {res},

                };
                if let Err(err) = res {
                    bail!("Error while solving: {}", err.to_string());
                }
                println!("Done");

                Ok(())
            }
            ReplCommand::Exit => unreachable!(), // Handled by the main loop
            ReplCommand::ShowHelp => {
                ReplCli::command().print_long_help()?;

                Ok(())
            }
        }
    }
}

fn load_project(path: &Path) -> anyhow::Result<Project> {
    let path = path.canonicalize().context("Failed to canonicalize path")?;
    info!("Opening file {}", path.as_os_str().to_string_lossy());
    let bytes = fs::read(path.clone()).context("Error reading file")?;

    toml::from_slice(&bytes).context("Error parsing file")
}

async fn run_solver(
    project: &Project,
    solver: SolverConfig,
    mut run_config: SolverRunConfig,
) -> anyhow::Result<()> {
    // Rather than a reverse for loop, this repeatedly looks for a solution with a
    // cardinality 1 lesser than each previous one. This means that, if there's a
    // high initial estimate, the SAT solver is likely to find a much more efficient
    // solution, and the solver doesn't step down by one each time unnecessarily.

    loop {
        // info!("Solving for n <= {}...", run_config.max_platforms());
        let (solver_future, interrupter) = solver.start(&run_config)?;

        let ctrl_c_cancellation = CancellationToken::new();
        tokio::spawn({
            let cancel = ctrl_c_cancellation.clone();
            async move {
                trace!(target: "solver_interrupter", "Interrupt listener ready");
                // TODO: better ctrl-c/interrupt handling
                // Example: on unix, this listener will continue to capture SIGINT even after
                // going out of scope, which might eat further SIGINTs if the program gets stuck
                // elsewhere etc.
                // Also, solver.start() takes a moment to run, and the user can press Ctrl-C
                // during that time
                // A global Ctrl-C listener that can cancel this whole task might be better

                match tokio::signal::ctrl_c().with_cancellation_token_owned(cancel).await {
                    None => {
                        trace!(target: "solver_interrupter", "Interrupt listener canceled");
                    }
                    Some(Ok(())) => {
                        println!("Aborting...");
                        interrupter.interrupt();
                    }
                    Some(Err(err)) => {
                        error!("Interrupt listener error: {err}");
                    }
                }
            }
        });
        let _guard = ctrl_c_cancellation.drop_guard();

        let sol = match solver_future.future().await? {
            SolverResponse::Sat(sol) => sol,
            SolverResponse::Unsat => {
                println!("No solution found for the current constraints");
                return Ok(());
            }
            SolverResponse::Aborted => {
                println!("Solver aborted");
                return Ok(());
            }
        };

        if sol.platform_count() == 0 {
            println!("Found a solution with no platforms - aborting");
            return Ok(());
        }

        run_config.limits_mut().entry(PLATFORMS_DEFAULT[0]).insert_entry(sol.platform_count() - 1);
        // todo!();

        println!("Solution found ({} platforms total)", sol.platform_count());
        let platform_stats = sol.platform_stats();
        for (def, count) in platform_stats.iter() {
            println!("{}: {}", def.dimensions_str(), count);
        }
        let validation = sol.validate(&project.world);
        if validation.overlapping_platforms.is_empty() && validation.unsupported_terrain.is_empty()
        {
            info!("Solution validation OK");
        } else {
            if !validation.overlapping_platforms.is_empty() {
                warn!(
                    "Validation failed: overlapping platforms:\n{}",
                    validation
                        .overlapping_platforms
                        .iter()
                        .map(|plat| format!(
                            "{}x{} at ({:>3};{:>3})",
                            plat.dims().width,
                            plat.dims().height,
                            plat.point().x,
                            plat.point().y
                        ))
                        .join("\n")
                );
            }

            if !validation.unsupported_terrain.is_empty() {
                warn!(
                    "Validation failed: Unsupported terrain:\n{}",
                    validation
                        .unsupported_terrain
                        .iter()
                        .map(|point| format!("({:>3};{:>3})", point.x, point.y))
                        .chunks(10)
                        .into_iter()
                        .map(|mut chunk| chunk.join(", "))
                        .join("\n")
                );
            }
        }

        print_world(&project.world, Some(&sol), &validation);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    repl_loop().await?;

    // Earlier logging code
    // {
    //     fs::create_dir_all("logs/")?;
    //     let path = format!("logs/{run_timestamp}_vars.log");
    //     info!("Writing variable map to {path}");
    //     let mut file = File::create_new(&path)?;
    //     solver.vars().write_var_map(&mut file)?;
    //
    //     let path = format!("logs/{run_timestamp}_dimacs.log");
    //     info!("Writing DIMACS to {path}");
    //     let mut file = File::create(&path)?;
    //     solver.instance().write_dimacs(&mut file)?
    // }
    Ok(())
}

fn print_world(world: &World, solution: Option<&Solution>, validation: &ValidationResult) {
    let terrain_grid = world.grid();
    let dims = terrain_grid.dims();

    #[derive(Copy, Clone, Debug)]
    enum Tile {
        Empty,
        Terrain,
        Platform(PlatformTile),
    }

    #[derive(Copy, Clone, Debug)]
    struct PlatformTile {
        north_edge: bool,
        south_edge: bool,
        west_edge: bool,
        east_edge: bool,
        overlapping: bool,
    }

    let mut tile_grid = Grid::new_fill(dims, Tile::Empty);

    for p in terrain_grid.enumerate().filter_map(|(p, val)| val.then_some(p)) {
        tile_grid.set(p, Tile::Terrain).unwrap();
    }

    if let Some(solution) = solution {
        for platform in solution.platforms().values() {
            let offset = platform.point();

            let dims = platform.dims();
            for rel_point in dims.iter_within() {
                let tile = PlatformTile {
                    north_edge: rel_point.y == 0,
                    south_edge: rel_point.y == (dims.height - 1) as isize,
                    west_edge: rel_point.x == 0,
                    east_edge: rel_point.x == (dims.width - 1) as isize,
                    overlapping: validation.overlapping_platforms.contains(platform),
                };
                // Pass if out of bounds
                _ = tile_grid.set(rel_point + offset, Tile::Platform(tile));
            }
        }
    }

    let fill_platform_middle = false;

    for row in tile_grid.iter_rows() {
        for t in row {
            let tile_str = match *t {
                Tile::Empty => " ".to_string(),
                Tile::Terrain => block_char::MEDIUM_SHADE.to_string(),
                Tile::Platform(PlatformTile {
                    north_edge: mut n,
                    south_edge: mut s,
                    west_edge: mut w,
                    east_edge: mut e,
                    overlapping,
                }) => {
                    // NSWE are true if there's _empty space_ in that direction
                    // The box chars function expects the opposite - where to connect to

                    // TODO Probably clean this up somehow
                    // Special case for 1x1
                    let out = if n && s && w && e {
                        '☐'.to_string()
                    } else {
                        // If only the middle should be filled, make it draw only the outline
                        if !fill_platform_middle {
                            match (n, s, w, e) {
                                (false, false, false, false) => {
                                    // Internal tile - make it empty instead
                                    (n, s, w, e) = (true, true, true, true);
                                }
                                (false, false, _, _) => {
                                    // NS connected - discard WE
                                    (w, e) = (true, true);
                                }
                                (_, _, false, false) => {
                                    // WE connected - discard NS
                                    (n, s) = (true, true);
                                }
                                _ => {}
                            };
                        }
                        box_char::by_adjacency_nswe(!n, !s, !w, !e).to_string()
                    };

                    if overlapping { out.red().to_string() } else { out }
                }
            };
            print!("{tile_str}  ");
        }
        println!();
    }
}

fn load_toml(path: PathBuf) -> anyhow::Result<Project> {
    println!(
        "Opening file {}",
        path.canonicalize().context("failed to canonicalize path")?.as_os_str().to_string_lossy()
    );

    if let Some(ext) = path.extension() {
        if ext != "toml" {
            warn!("Loaded TOML file has unexpected extension: {}", ext.display());
        }
    } else {
        warn!("Loaded TOML file has no extension");
    };

    let bytes = fs::read(path).context("Failed to read file")?;
    toml::from_slice(&bytes).context("Failed to parse TOML file")
}

fn print_grid<T>(grid: &Grid<T>, map_fn: impl Fn(&T) -> char) {
    for row in grid.iter_rows() {
        row.iter().map(&map_fn).for_each(|x| print!("{x}  "));
        println!();
    }
}

mod block_char {
    pub const FULL: char = '\u{2588}';
    pub const LIGHT_SHADE: char = '\u{2591}';
    pub const MEDIUM_SHADE: char = '\u{2592}';
}

mod box_char {
    const CHARS: [char; 16] = [
        // Order: NSWE, E is LSb
        ' ', // none
        '╶', // E
        '╴', // W
        '─', // WE
        '╷', // S
        '┌', // SE
        '┐', // SW
        '┬', // SWE
        '╵', // N
        '└', // NE
        '┘', // NW
        '┴', // NWE
        '│', // NS
        '├', // NSE
        '┤', // NSW
        '┼', // NSWE
    ];

    pub const fn by_adjacency_nswe(north: bool, south: bool, west: bool, east: bool) -> char {
        let index =
            (north as usize) << 3 | (south as usize) << 2 | (west as usize) << 1 | (east as usize);
        CHARS[index]
    }
}
