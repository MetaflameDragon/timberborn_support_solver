#![allow(dead_code)]

use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    fs::File,
    io::Write,
    num::ParseIntError,
    ops::ControlFlow,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, bail, ensure};
use assertables::{assert_gt, assert_le};
use clap::{
    Arg, ArgAction, Command, CommandFactory, Error, FromArgMatches, Parser, Subcommand,
    builder::{TypedValueParser, ValueParserFactory},
    value_parser,
};
use log::{error, info, warn};
use rustsat::solvers::InterruptSolver;
use serde::Deserialize;
use thiserror::Error;
use timberborn_support_solver::{
    PlatformLimits, Project, Solution, SolverConfig, SolverResult, SolverRunConfig,
    dimensions::{DimTy, Dimensions},
    grid::Grid,
    platform::{Platform, PlatformType},
    point::Point,
    utils::loop_with_feedback,
    world::{World, WorldGrid},
};

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
    /// Solve platform placement for the currently loaded project
    #[command(visible_aliases = ["s"])]
    Solve {
        /// Limits for platform types
        ///
        /// Limits are specified as `k:v` key-value pairs.
        /// Multiple limits may be separated by commas, or the flag may be
        /// specified multiple times.
        ///
        /// Example: `-l5:2,3:4 -l1:10`
        /// ~ At most 2 5x5 platforms, at most 4 3x3 or larger, at most 10 1x1 or larger.
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
    Duplicate(PlatformType),
}

fn try_into_platform_limits(
    limit_args: Vec<PlatformLimitArg>,
) -> Result<PlatformLimits, PlatformLimitError> {
    let mut map = HashMap::new();
    for PlatformLimitArg(ty, count) in limit_args.into_iter() {
        if map.insert(ty, count).is_some() {
            // Duplicate value
            // Maybe needs a better error type? it's just one error path for now though
            return Err(PlatformLimitError::Duplicate(ty));
        }
    }

    Ok(PlatformLimits::new(map))
}

#[derive(Clone, Debug)]
struct PlatformLimitArg(PlatformType, usize);

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
        let r#type = match key.trim() {
            "5" => PlatformType::Square5x5,
            "3" => PlatformType::Square3x3,
            "1" => PlatformType::Square1x1,
            other => return Err(Error::InvalidType(other.to_string())),
        };

        let count: usize = value.trim().parse().map_err(|err| Error::InvalidValue(err))?;

        Ok(PlatformLimitArg(r#type, count))
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

type InterrupterContainer = Arc<Mutex<Option<Box<dyn InterruptSolver + Send>>>>;

async fn repl_loop() -> anyhow::Result<()> {
    ReplCli::command().print_long_help()?;

    struct State {
        loaded_project: Option<LoadedProject>,
    }

    struct LoadedProject {
        project: Project,
        path: PathBuf,
    }
    let mut state = State { loaded_project: None };

    loop {
        if let Some(LoadedProject { project: _project, path }) = &state.loaded_project {
            // Try to show just the file name, fall back to the whole path
            let proj_path_str = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
            info!("Currently loaded project: {}", proj_path_str);
        };

        let cli = match parse_repl() {
            Ok(cli) => cli,
            Err(ReplParseError::Empty) => {
                ReplCli::command().print_long_help()?;
                continue;
            }
            Err(err) => {
                error!("{}", err.to_string());
                continue;
            }
        };
        let cmd = cli.cmd;

        if matches!(cmd, ReplCommand::Exit) {
            return Ok(());
        }

        if let Err(err) = run_cmd(cmd, &mut state).await {
            error!("{}", err.to_string());
        }
    }

    async fn run_cmd(cmd: ReplCommand, state: &mut State) -> anyhow::Result<()> {
        match cmd {
            ReplCommand::Load { path } => {
                state.loaded_project = Some(LoadedProject { project: load_project(&path)?, path });
                info!("Loaded");

                Ok(())
            }
            ReplCommand::Reload => {
                let Some(LoadedProject { path, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                state.loaded_project =
                    Some(LoadedProject { project: load_project(&path)?, path: path.clone() });
                info!("Loaded");

                Ok(())
            }
            ReplCommand::Terrain => {
                let Some(LoadedProject { project, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                print_world(&project.world, None);

                Ok(())
            }
            ReplCommand::Solve { limits } => {
                let limits = try_into_platform_limits(limits)?;
                let Some(LoadedProject { project, .. }) = &state.loaded_project else {
                    bail!("No project loaded");
                };

                let solver = SolverConfig::new(&project.world);
                let run_config = SolverRunConfig { limits };

                if let Err(err) = run_solver(&project, solver, run_config).await {
                    bail!("Error while solving: {}", err.to_string());
                }
                info!("Done");

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
        let sol = match solver.start(&run_config)?.await?? {
            SolverResult::Sat(sol) => sol,
            SolverResult::Unsat => {
                info!("No solution found for the current constraints");
                return Ok(());
            }
            SolverResult::Aborted => {
                info!("Aborted");
                return Ok(());
            }
            SolverResult::Error(err) => return Err(err),
        };

        if sol.platform_count() == 0 {
            info!("Found a solution with no platforms - aborting");
            return Ok(());
        }
        // assert_gt!(sol.platform_count(), 0, "Solution should have at least one
        // platform");
        run_config.limits_mut().insert(PlatformType::Square1x1, sol.platform_count() - 1);

        info!("Solution found ({} platforms total)", sol.platform_count());
        let platform_stats = sol.platform_stats();
        for (ty, count) in platform_stats.iter() {
            info!("{}: {}", ty.dimensions_str(), count);
        }
        print_world(&project.world, Some(&sol));
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    // let run_timestamp = chrono::Utc::now().format(r"%y%m%d_%H%M%S");

    // TODO: reimplement interrupter once it's supported
    // let interrupter: InterrupterContainer = Arc::new(Mutex::new(None));
    //
    // if let Err(err) = ctrlc::set_handler({
    //     let interrupter = interrupter.clone();
    //     let mut is_repeat = false;
    //     move || {
    //         if is_repeat {
    //             warn!("Aborting immediately");
    //             std::process::exit(-1);
    //         }
    //
    //         is_repeat = true;
    //         warn!("Stopping...");
    //         if let Some(int) = &*interrupter.lock().expect("Mutex was poisoned!")
    // {             int.interrupt();
    //         }
    //     }
    // }) {
    //     warn!("Failed to set interrupt handler! {err}");
    // }

    repl_loop().await?;

    // {
    //     // TODO really dirty, clean up logging
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

fn print_world(world: &World, solution: Option<&Solution>) {
    let terrain_grid = world.grid();
    let dims = terrain_grid.dims();

    let mut char_grid = Grid::new_fill(dims, ' ');

    for p in terrain_grid.enumerate().filter_map(|(p, val)| val.then_some(p)) {
        char_grid.set(p, block_char::MEDIUM_SHADE).unwrap();
    }

    if let Some(solution) = solution {
        for (point, platform) in solution.platforms() {
            let platform = Platform::new(*point, *platform);
            let (lower, upper) = platform.area_corners();

            let fill = match platform.platform_type() {
                PlatformType::Square1x1 => '1',
                PlatformType::Square3x3 => '3',
                PlatformType::Square5x5 => '5',
            };

            for y in lower.y..=upper.y {
                for x in lower.x..=upper.x {
                    let q = Point::new(x, y);
                    // Pass if out of bounds
                    _ = char_grid.set(q, fill);
                }
            }
        }
    }

    for row in char_grid.iter_rows() {
        for c in row {
            print!("{}  ", c);
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
