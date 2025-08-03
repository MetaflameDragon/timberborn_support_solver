#![allow(dead_code)]

use std::{
    fs,
    fs::File,
    io::Write,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, bail};
use assertables::{assert_gt, assert_le};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use log::{info, warn};
use rustsat::solvers::InterruptSolver;
use timberborn_support_solver::{
    Solution, SolverConfig, SolverResult, SolverRunConfig,
    dimensions::{DimTy, Dimensions},
    grid::Grid,
    platform::{Platform, PlatformType},
    point::Point,
    utils::loop_with_feedback,
    world::World,
};

#[derive(Parser)]
struct Cli {
    /// Initial support platform count to aim for (a high estimate).
    start_count: usize,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Calculates a solution for a filled rectangular area.
    Rect { width: DimTy, height: DimTy },
    /// Takes a file as the ceiling layout - each line is a row,
    /// 'X' marks ceiling blocks, '.' or ' ' marks empty space.
    File { path: PathBuf },
}

fn parse_or_readline() -> anyhow::Result<Cli> {
    // Args were provided (try to parse, exit on fail)
    if std::env::args_os().len() > 1 {
        return Ok(Cli::parse());
    }

    let mut cmd = Cli::command().no_binary_name(true);

    println!("No CLI arguments were provided");
    println!("Specify arguments via stdin:");
    println!("{}", cmd.render_long_help());

    std::io::stdout().flush().context("could not write to stdout")?;
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer).context("could not read stdin")?;

    let args = shlex::split(buffer.trim()).context("invalid quoting")?;
    let matches = cmd.try_get_matches_from(args).context("failed to parse args")?;

    Cli::from_arg_matches(&matches).context("failed to parse args")
}

type InterrupterContainer = Arc<Mutex<Option<Box<dyn InterruptSolver + Send>>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let run_timestamp = chrono::Utc::now().format(r"%y%m%d_%H%M%S");

    let interrupter: InterrupterContainer = Arc::new(Mutex::new(None));

    if let Err(err) = ctrlc::set_handler({
        let interrupter = interrupter.clone();
        let mut is_repeat = false;
        move || {
            if is_repeat {
                warn!("Aborting immediately");
                std::process::exit(-1);
            }

            is_repeat = true;
            warn!("Stopping...");
            // TODO: handle stdin somehow?
            if let Some(int) = &*interrupter.lock().expect("Mutex was poisoned!") {
                int.interrupt();
            }
        }
    }) {
        warn!("Failed to set interrupt handler! {err}");
    }
    let args = parse_or_readline()?;

    let terrain_grid: Grid<bool> = match args.cmd {
        Command::Rect { width, height } => Grid::new_fill(Dimensions::new(width, height), true),
        Command::File { path } => load_grid_from_file(path)?,
    };

    let world = World::new(terrain_grid);

    let mut solver = SolverConfig::new(&world);

    {
        // TODO really dirty, clean up logging
        fs::create_dir_all("logs/")?;
        let path = format!("logs/{run_timestamp}_vars.log");
        info!("Writing variable map to {path}");
        let mut file = File::create_new(&path)?;
        solver.vars().write_var_map(&mut file)?;

        let path = format!("logs/{run_timestamp}_dimacs.log");
        info!("Writing DIMACS to {path}");
        let mut file = File::create(&path)?;
        solver.instance().write_dimacs(&mut file)?
    }

    // Rather than a reverse for loop, this repeatedly looks for a solution with a
    // cardinality 1 lesser than each previous one. This means that, if there's a
    // high initial estimate, the SAT solver is likely to find a much more efficient
    // solution, and the solver doesn't step down by one each time unnecessarily.

    let mut run_config = SolverRunConfig { max_cardinality: args.start_count };

    loop {
        info!("Solving for n <= {}...", run_config.max_cardinality);
        let sol = match solver.start(run_config)?.await?? {
            SolverResult::Sat(sol) => sol,
            SolverResult::Unsat => break,
            SolverResult::Aborted => break,
            SolverResult::Error(err) => return Err(err),
        };

        assert_gt!(sol.platform_count(), 0, "Solution should have at least one platform");
        run_config.max_cardinality = sol.platform_count() - 1;

        info!("Solution: ({} platforms)", sol.platform_count());
        print_solution(&world, &sol);
    }
    Ok(())
}

fn print_solution(world: &World, solution_data: &Solution) {
    let terrain_grid = world.terrain_grid();
    let dims = terrain_grid.dims();

    let mut char_grid = Grid::new_fill(dims, ' ');

    for p in terrain_grid.enumerate().filter_map(|(p, val)| val.then_some(p)) {
        char_grid.set(p, block_char::MEDIUM_SHADE).unwrap();
    }

    for (point, platform) in solution_data.platforms() {
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

    for row in char_grid.iter_rows() {
        for c in row {
            print!("{}  ", c);
        }
        println!();
    }
}

fn load_grid_from_file(path: PathBuf) -> anyhow::Result<Grid<bool>> {
    println!(
        "Opening file {}",
        path.canonicalize().context("failed to canonicalize path")?.as_os_str().to_string_lossy()
    );

    let file_str = std::fs::read_to_string(path).context("Failed to read file")?;

    let lines: Vec<_> = file_str.lines().collect();
    if lines.is_empty() {
        bail!("File is empty");
    }

    let dims = Dimensions::new(
        lines.iter().map(|line| line.chars().count()).max().unwrap() as DimTy,
        lines.len() as DimTy,
    );

    let grid_flat: Vec<bool> = lines
        .iter()
        .map(|line| -> anyhow::Result<Vec<bool>> {
            let mut line: Vec<_> = line
                .chars()
                .map(|c| match c {
                    ' ' => Ok(false),
                    'X' => Ok(true),
                    c => {
                        bail!("invalid character '{}' (expected 'X' or ' ')", c);
                    }
                })
                .collect::<Result<_, _>>()?;
            line.resize(dims.width as usize, false);
            Ok(line)
        })
        .collect::<Result<Vec<Vec<bool>>, _>>()?
        .into_iter()
        .flatten()
        .collect();

    Grid::try_from_vec(dims, grid_flat).context("Failed to create grid")
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
