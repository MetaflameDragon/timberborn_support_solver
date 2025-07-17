#![allow(dead_code)]

use std::{
    array,
    collections::{HashMap, HashSet, VecDeque},
    io::Write,
    iter,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use dimensions::Dimensions;
use grid::Grid;
use log::{error, info, warn};
use rustsat::{
    OutOfMemory,
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, ManageVars, SatInstance},
    solvers::{Interrupt, InterruptSolver, Solve, SolverResult},
    types::{Assignment, Clause, Lit, Var, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use thiserror::Error;

use crate::{dimensions::DimTy, point::Point};

mod dimensions;
mod grid;
mod point;

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

fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let interrupter: Arc<Mutex<Option<Box<dyn InterruptSolver + Send>>>> =
        Arc::new(Mutex::new(None));

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
        warn!("Failed to set interrupt handler! {}", err);
    }
    let args = parse_or_readline()?;

    let mut instance: SatInstance<BasicVarManager> = SatInstance::new();

    let terrain_grid: Grid<bool> = match args.cmd {
        Command::Rect { width, height } => Grid::new_fill(Dimensions::new(width, height), true),
        Command::File { path } => load_grid_from_file(path)?,
    };

    #[derive(Debug, Default, Clone)]
    struct PlatformVars {
        p_1x1: HashMap<Point, Var>,
        p_3x3: HashMap<Point, Var>,
        p_5x5: HashMap<Point, Var>,
    }

    let mut platform_vars: PlatformVars = Default::default();

    // Add vars for platforms, and implication clauses between them
    for p in terrain_grid.dims().iter_within() {
        let vars_mgr = instance.var_manager_mut();
        let var_1 = vars_mgr.new_var();
        let var_3 = vars_mgr.new_var();
        let var_5 = vars_mgr.new_var();
        platform_vars.p_1x1.insert(p, var_1);
        platform_vars.p_3x3.insert(p, var_3);
        platform_vars.p_5x5.insert(p, var_5);

        // Implication clauses: larger => smaller
        // A smaller platform is always to be entirely contained within the larger one
        // Consequently, ~smaller => ~larger
        // i.e. if you can't place a smaller platform, you can't place a larger one
        // either
        instance.add_lit_impl_lit(var_5.pos_lit(), var_3.pos_lit());
        instance.add_lit_impl_lit(var_3.pos_lit(), var_1.pos_lit());
    }

    // Handle platform overlap
    // Plain clauses, with stated ranges != (x, y):
    //
    // P5(x, y) -> ~P1(x..=x+4, y..=y+4)
    // P5(x, y) -> ~P3(x-2..=x+4, y-2..=y+4)
    // P5(x, y) -> ~P5(x-4..=x+4, y-4..=y+4)
    //
    // P3(x, y) -> ~P1(x..=x+2, y..=y+2)
    // P3(x, y) -> ~P3(x-2..=x+2, y-2..=y+2)
    // TODO: simplify via implications P5(x,y) -> P3(x,y) -> P1(x,y) etc.

    // 5x5 <-> _
    for (&p, &var_5x5) in platform_vars.p_5x5.iter() {
        // 5x5 <-> 1x1
        for x in p.x..=(p.x + 4) {
            for y in p.y..=(p.y + 4) {
                let p_other = Point::new(x, y);
                if p == p_other {
                    continue;
                }
                let Some(var_1x1) = platform_vars.p_1x1.get(&p_other).copied() else { continue };

                instance.add_lit_impl_lit(var_5x5.pos_lit(), var_1x1.neg_lit());
            }
        }

        // 5x5 <-> 3x3
        for x in (p.x - 2)..=(p.x + 4) {
            for y in (p.y - 2)..=(p.y + 4) {
                let p_other = Point::new(x, y);
                if p == p_other {
                    continue;
                }
                let Some(var_3x3) = platform_vars.p_3x3.get(&p_other).copied() else { continue };

                instance.add_lit_impl_lit(var_5x5.pos_lit(), var_3x3.neg_lit());
            }
        }

        // 5x5 <-> 5x5
        for x in (p.x - 4)..=(p.x + 4) {
            for y in (p.y - 4)..=(p.y + 4) {
                let p_other = Point::new(x, y);
                if p == p_other {
                    continue;
                }
                let Some(var_5x5_b) = platform_vars.p_5x5.get(&p_other).copied() else { continue };

                instance.add_lit_impl_lit(var_5x5.pos_lit(), var_5x5_b.neg_lit());
            }
        }
    }

    // 3x3 <-> _
    for (&p, &var_3x3) in platform_vars.p_3x3.iter() {
        // 3x3 <-> 1x1
        for x in p.x..=(p.x + 2) {
            for y in p.y..=(p.y + 2) {
                let p_other = Point::new(x, y);
                if p == p_other {
                    continue;
                }
                let Some(var_1x1) = platform_vars.p_1x1.get(&p_other).copied() else { continue };

                instance.add_lit_impl_lit(var_3x3.pos_lit(), var_1x1.neg_lit());
            }
        }

        // 3x3 <-> 3x3
        for x in (p.x - 2)..=(p.x + 2) {
            for y in (p.y - 2)..=(p.y + 2) {
                let p_other = Point::new(x, y);
                if p == p_other {
                    continue;
                }
                let Some(var_3x3_b) = platform_vars.p_3x3.get(&p_other).copied() else { continue };

                instance.add_lit_impl_lit(var_3x3.pos_lit(), var_3x3_b.neg_lit());
            }
        }
    }

    //
    // let point_map =
    //     build_clauses(&mut instance, &terrain_grid).context("failed to build
    // clauses")?;

    // Terrain support goal clauses
    const TERRAIN_SUPPORT_DISTANCE: usize = 3;
    // Terrain support distance is handled in "layers", like variables stacked on
    // top of one another
    // Tiles from each layer imply their neighbors in the next
    // All tiles in the topmost layer are goals
    // All tiles in the bottom-most layer are implied by
    // the platforms supporting them
    let mut terrain_vars: HashMap<Point, [Var; TERRAIN_SUPPORT_DISTANCE]> = Default::default();

    // New vars for all occupied spaces in the terrain grid
    for p in terrain_grid.enumerate().filter_map(|(p, &v)| v.then_some(p)) {
        terrain_vars.insert(p, array::from_fn(|_| instance.new_var()));
    }

    for (p, vars) in &terrain_vars {
        // Layer implication clauses
        for lower_i in 0..(TERRAIN_SUPPORT_DISTANCE - 1) {
            let upper_i = lower_i + 1;

            // Support the tile directly above
            let lower_var = vars[lower_i];
            let upper_var = vars[upper_i];
            instance.add_lit_impl_lit(lower_var.pos_lit(), upper_var.pos_lit());

            // Support all neighbors
            for q in p.neighbors() {
                if let Some(neighbor_column) = terrain_vars.get(&q) {
                    let upper_var = neighbor_column[upper_i];
                    instance.add_lit_impl_lit(lower_var.pos_lit(), upper_var.pos_lit());
                }
            }
        }

        // Require the topmost layer
        instance.add_unit(vars.last().unwrap().pos_lit());
    }

    // 1x1 supports for terrain
    for (p, platform_var) in &platform_vars.p_1x1 {
        if let Some(tile_vars) = terrain_vars.get(p) {
            let tile_var = tile_vars.first().unwrap();
            instance.add_lit_impl_lit(platform_var.pos_lit(), tile_var.pos_lit());
        }
    }

    // 3x3 supports for terrain
    for (p, platform_var) in &platform_vars.p_3x3 {
        for x in p.x..=(p.x + 2) {
            for y in p.y..=(p.y + 2) {
                let q = Point::new(x, y);
                if let Some(tile_vars) = terrain_vars.get(&q) {
                    let tile_var = tile_vars.first().unwrap();
                    instance.add_lit_impl_lit(platform_var.pos_lit(), tile_var.pos_lit());
                }
            }
        }
    }

    // 5x5 supports for terrain
    for (p, platform_var) in &platform_vars.p_5x5 {
        for x in p.x..=(p.x + 4) {
            for y in p.y..=(p.y + 4) {
                let q = Point::new(x, y);
                if let Some(tile_vars) = terrain_vars.get(&q) {
                    let tile_var = tile_vars.first().unwrap();
                    instance.add_lit_impl_lit(platform_var.pos_lit(), tile_var.pos_lit());
                }
            }
        }
    }

    let mut max_cardinality = args.start_count;

    // Rather than a reverse for loop, this repeatedly looks for a solution with a
    // cardinality 1 lesser than each previous one. This means that, if there's a
    // high initial estimate, the SAT solver is likely to find a much more efficient
    // solution, and the solver doesn't step down by one each time unnecessarily.
    while max_cardinality > 0 {
        let mut solver = GlucoseSimp::default();
        *interrupter.lock().expect("Mutex was poisoned!") = Some(Box::new(solver.interrupter()));

        println!("Solving for n <= {max_cardinality}...");

        let upper_constraint = CardConstraint::new_ub(
            platform_vars.p_1x1.values().map(|var| var.pos_lit()),
            max_cardinality,
        );

        let assignment = match try_solve(&mut solver, instance.clone(), upper_constraint) {
            Ok(sol) => sol,
            Err(SolveError::Unsat) => {
                println!("Unsat");
                break;
            }
            Err(SolveError::Interrupted) => {
                println!("Interrupted!");
                break;
            }
            Err(SolveError::Other(err)) => {
                bail!(err)
            }
        };

        let mut sol: Solution = Default::default();

        for (&p, &var) in &platform_vars.p_1x1 {
            if assignment.var_value(var).to_bool_with_def(false) {
                sol.platforms.insert(p, PlatformType::Square1x1);
            }
        }
        for (&p, &var) in &platform_vars.p_3x3 {
            if assignment.var_value(var).to_bool_with_def(false) {
                sol.platforms.insert(p, PlatformType::Square3x3);
            }
        }
        for (&p, &var) in &platform_vars.p_5x5 {
            if assignment.var_value(var).to_bool_with_def(false) {
                sol.platforms.insert(p, PlatformType::Square5x5);
            }
        }

        // let supports_grid = Grid::from_map(terrain_grid.dims(), |p| {
        //     sol.var_value(*point_map.get(&p).unwrap()).to_bool_with_def(false)
        // });

        let marked_count = sol.platforms.iter().count();
        println!("Solution: ({marked_count} marked)");

        max_cardinality = marked_count - 1;

        println!("{sol:#?}");

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
        //             "A support was incorrectly placed without terrain above
        // it (at {})",             p
        //         ),
        //     }
        // });
        //
        // print_grid(&combined_grid, |b| match b {
        //     None => block_char::LIGHT_SHADE,
        //     Some(Tile::Terrain) => block_char::MEDIUM_SHADE,
        //     Some(Tile::Support) => block_char::FULL,
        // });
    }

    Ok(())
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
enum PlatformType {
    Square1x1,
    Square3x3,
    Square5x5,
}

#[derive(Clone, Debug, Default)]
struct Solution {
    platforms: HashMap<Point, PlatformType>,
}

#[derive(Error, Debug)]
enum SolveError {
    #[error("unsatisfiable")]
    Unsat,
    #[error("interrupted")]
    Interrupted,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

fn try_solve(
    solver: &mut (impl Solve + rustsat::solvers::SolveStats),
    instance: SatInstance<BasicVarManager>,
    card_constraint: CardConstraint,
) -> Result<Assignment, SolveError> {
    let (cnf, mut var_manager) = instance.into_cnf();

    solver.add_cnf(cnf).context("Failed to add clause")?;

    card::encode_cardinality_constraint::<Totalizer, _>(card_constraint, solver, &mut var_manager)
        .context("failed to encode cardinality constraint")?;

    match solver.solve().context("error while solving")? {
        SolverResult::Sat => Ok(solver.full_solution().context("Failed to get full solution")?),
        SolverResult::Unsat => Err(SolveError::Unsat),
        SolverResult::Interrupted => Err(SolveError::Interrupted),
    }
}

fn build_clauses(
    instance: &mut SatInstance<BasicVarManager>,
    terrain_grid: &Grid<bool>,
) -> Result<HashMap<Point, Var>, OutOfMemory> {
    let mut point_lit_map = HashMap::new();

    let var_manager = instance.var_manager_mut();
    // Reserve vars for all points on the grid
    for p in terrain_grid.dims().iter_within() {
        let var = var_manager.new_var();
        point_lit_map.insert(p, var);
    }

    for (point, val) in terrain_grid.enumerate() {
        if !val {
            continue;
        }
        let clause: Clause = Clause::from_iter(
            point.adjacent_points(3, terrain_grid).into_iter().map(|p| point_lit_map[&p].pos_lit()),
        );
        instance.add_clause(clause);
    }
    Ok(point_lit_map)
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
