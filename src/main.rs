use crate::{dimensions::DimTy, point::Point};
use anyhow::{Context, bail};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use dimensions::Dimensions;
use grid::Grid;
use rustsat::{
    OutOfMemory,
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, ManageVars, SatInstance},
    solvers::{Solve, SolverResult},
    types::{Assignment, Clause, Var, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use std::{collections::HashMap, io::Write, path::PathBuf};
use thiserror::Error;

mod dimensions;
mod grid;
mod point;

#[derive(Parser)]
struct Cli {
    start_count: usize,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Rect { width: DimTy, height: DimTy },
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
    println!("{}", cmd.render_usage());

    std::io::stdout()
        .flush()
        .context("could not write to stdout")?;
    let mut buffer = String::new();
    std::io::stdin()
        .read_line(&mut buffer)
        .context("could not read stdin")?;

    let args = shlex::split(buffer.trim()).context("invalid quoting")?;
    let matches = cmd
        .try_get_matches_from(args)
        .context("failed to parse args")?;

    Ok(Cli::from_arg_matches(&matches).context("failed to parse args")?)
}

fn main() -> anyhow::Result<()> {
    let args = parse_or_readline()?;

    let mut instance: SatInstance<BasicVarManager> = SatInstance::new();

    let terrain_grid: Grid<bool> = match args.cmd {
        Command::Rect { width, height } => Grid::new_fill(Dimensions::new(width, height), true),
        Command::File { path } => load_grid_from_file(path)?,
    };

    let point_map =
        build_clauses(&mut instance, &terrain_grid).context("failed to build clauses")?;

    for max_cardinality in (1..=args.start_count).rev() {
        let mut solver = GlucoseSimp::default();

        println!("Solving for n <= {max_cardinality}...");
        let sol = match try_solve(&mut solver, instance.clone(), max_cardinality, &point_map) {
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

        let supports_grid = Grid::from_map(terrain_grid.dims(), |p| {
            sol.var_value(*point_map.get(&p).unwrap())
                .to_bool_with_def(false)
        });

        let marked_count = supports_grid.iter().filter(|&&x| x).count();
        println!("Solution: ({marked_count} marked)");

        enum Tile {
            Support,
            Terrain,
        }

        assert_eq!(terrain_grid.dims(), supports_grid.dims());
        let combined_grid: Grid<Option<Tile>> = Grid::from_map(terrain_grid.dims(), |p| {
            match (
                terrain_grid.get(p).copied().unwrap(),
                supports_grid.get(p).copied().unwrap(),
            ) {
                (true, true) => Some(Tile::Support),
                (true, false) => Some(Tile::Terrain),
                (false, false) => None,
                (false, true) => unreachable!(
                    "A support was incorrectly placed without terrain above it (at {})",
                    p
                ),
            }
        });

        print_grid(&combined_grid, |b| match b {
            None => block_char::LIGHT_SHADE,
            Some(Tile::Terrain) => block_char::MEDIUM_SHADE,
            Some(Tile::Support) => block_char::FULL,
        });
    }

    Ok(())
}

fn load_grid_from_file(path: PathBuf) -> anyhow::Result<Grid<bool>> {
    println!(
        "Opening file {}",
        path.canonicalize()
            .context("failed to canonicalize path")?
            .as_os_str()
            .to_string_lossy()
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
    max_cardinality: usize,
    point_map: &HashMap<Point, Var>,
) -> Result<Assignment, SolveError> {
    let (cnf, mut var_manager) = instance.into_cnf();

    solver.add_cnf(cnf).context("Failed to add clause")?;

    let upper_constraint =
        CardConstraint::new_ub(point_map.values().map(|var| var.pos_lit()), max_cardinality);

    card::encode_cardinality_constraint::<Totalizer, _>(upper_constraint, solver, &mut var_manager)
        .context("failed to encode cardinality constraint")?;

    match solver.solve().context("error while solving")? {
        SolverResult::Sat => Ok(solver
            .full_solution()
            .context("Failed to get full solution")?),
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
    for p in terrain_grid.dims().iter_within() {
        let var = var_manager.new_var();
        point_lit_map.insert(p, var);
    }

    for (point, val) in terrain_grid.enumerate() {
        if !val {
            continue;
        }
        let clause: Clause = Clause::from_iter(
            point
                .iter_within_manhattan(3)
                .filter(|p| terrain_grid.dims().contains(*p))
                .map(|p| point_lit_map[&p].pos_lit()),
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
