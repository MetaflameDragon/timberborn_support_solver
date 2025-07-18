#![allow(dead_code)]

use std::{
    fs::File,
    io::Write,
    ops::ControlFlow,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, bail};
use assertables::{assert_gt, assert_le};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use dimensions::Dimensions;
use grid::Grid;
use log::{info, warn};
use rustsat::solvers::InterruptSolver;
use world::World;

use crate::{
    dimensions::DimTy,
    platform::{Platform, PlatformType},
    point::Point,
    solver::{SolutionData, SolveError, Solver},
};

mod dimensions;
mod grid;
mod platform;
mod point;
mod solver;
mod world;

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

fn main() -> anyhow::Result<()> {
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

    let mut solver = Solver::new(&world);

    {
        let path = format!("{run_timestamp}_vars.log");
        info!("Writing variable map to {path}");
        let mut file = File::create_new(&path)?;
        solver.vars().write_var_map(&mut file)?;

        let path = format!("{run_timestamp}_dimacs.log");
        info!("Writing DIMACS to {path}");
        let mut file = File::create(&path)?;
        solver.instance().write_dimacs(&mut file)?
    }

    // Rather than a reverse for loop, this repeatedly looks for a solution with a
    // cardinality 1 lesser than each previous one. This means that, if there's a
    // high initial estimate, the SAT solver is likely to find a much more efficient
    // solution, and the solver doesn't step down by one each time unnecessarily.

    struct SolverParams {
        max_cardinality: usize,
    }
    struct SolverResult {
        solution: SolutionData, // TODO: try to make this a Solution tied to the World by lifetime
    }

    loop_with_feedback(
        SolverParams { max_cardinality: args.start_count },
        |_, result: SolverResult| {
            ControlFlow::Continue(SolverParams {
                max_cardinality: result.solution.platform_count() - 1,
            })
        },
        |_i, SolverParams { max_cardinality }| {
            info!("Solving for n <= {max_cardinality}...");
            let solution = match (&mut solver).solve(interrupter.clone(), max_cardinality) {
                Ok(Some(sol)) => sol,
                Ok(None) => {
                    info!("No solution found for n <= {max_cardinality} (UNSAT)");
                    return ControlFlow::Break(Ok(()));
                }
                Err(SolveError::Interrupted) => {
                    warn!("Interrupted!");
                    return ControlFlow::Break(Ok(()));
                }
                Err(SolveError::Other(err)) => {
                    return ControlFlow::Break(Err(err));
                }
            };

            info!("Solution: ({} platforms)", solution.platform_count());

            assert_gt!(solution.platform_count(), 0, "A solution should not have 0 platforms!");
            assert_le!(
                solution.platform_count(),
                max_cardinality,
                "The solution had more marked platforms than expected!"
            );
            println!("{:#?}", solution.platforms());

            print_solution(&world, &solution);

            ControlFlow::Continue(SolverResult { solution })

            // TODO?
            // {
            //     let path =
            //         format!("
            // {run_timestamp}_assignment_{run_i}_ub_{max_cardinality}.log"
            //         );     info!("Writing assignment to {path}");
            //     let mut file = File::create_new(&path)?;
            //
            //     for lit in assignment.iter() {
            //         writeln!(
            //             &mut file,
            //             "{} | {}",
            //             lit,
            //
            // var_repr_map.get(&lit.var()).unwrap_or(&"?".to_owned())
            //         )?;
            //     }
            // }

            //
            // enum Tile {
            //     Support,
            //     Terrain,
            // }
            //
            // assert_eq!(terrain_grid.dims(), supports_grid.dims());
            // let combined_grid: Grid<Option<Tile>> =
            // Grid::from_map(terrain_grid.dims(), |p| {
            //     match (terrain_grid.get(p).copied().unwrap(),
            // supports_grid.get(p).copied().unwrap()) {         (true,
            // true) => Some(Tile::Support),         (true, false) =>
            // Some(Tile::Terrain),         (false, false) => None,
            //         (false, true) => unreachable!(
            //             "A support was incorrectly placed without terrain
            // above it (at {})",             p
            //         ),
            //     }
            // });
            //
            // print_grid(&combined_grid, |b| match b {
            //     None => block_char::LIGHT_SHADE,
            //     Some(Tile::Terrain) => block_char::MEDIUM_SHADE,
            //     Some(Tile::Support) => block_char::FULL,
            // });
        },
    )?;

    Ok(())
}

/// Repeats a closure similar to a `for` loop, advancing the state using another
/// closure based on feedback from the loop.
///
/// Both the called closure and the iterator closure can decide to stop the loop
/// by returning [`ControlFlow::Break`]. The iteration index is also provided
/// automatically. Both closures are [`FnMut`], so they can keep an internal
/// mutable state, too.
///
/// These closures are run in sequence, effectively no different from a loop
/// with iteration statements at the end, but this wrapper helps separate the
/// action and iteration. The main closure takes `T` as input, producing
/// [`ControlFlow<B, U>`][`ControlFlow`] as feedback ([`ControlFlow::Continue`]
/// to continue, [`ControlFlow::Break`] to terminate). The `after_each` iterator
/// then receives `U` as input, producing [`ControlFlow<B, T>`][`ControlFlow`]
/// again for the next iteration (or, again, returning [`ControlFlow::Break`] to
/// terminate).
fn loop_with_feedback<T, U, B, F, C>(initial: T, mut after_each: F, mut closure: C) -> B
where
    F: FnMut(usize, U) -> ControlFlow<B, T>,
    C: FnMut(usize, T) -> ControlFlow<B, U>,
{
    let mut input = initial;
    let mut iteration = 0;
    loop {
        let output = match closure(iteration, input) {
            ControlFlow::Continue(output) => output,
            ControlFlow::Break(result) => return result,
        };
        input = match after_each(iteration, output) {
            ControlFlow::Continue(input) => input,
            ControlFlow::Break(result) => return result,
        };

        iteration += 1;
    }
}

fn print_solution(world: &World, solution_data: &SolutionData) {
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
