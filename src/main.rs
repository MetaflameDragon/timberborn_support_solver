use crate::point::Point;
use anyhow::{bail, Context};
use dimensions::Dimensions;
use grid::Grid;
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, ManageVars, SatInstance},
    solvers::{Solve, SolverResult},
    types::{constraints::CardConstraint, Assignment, Clause, Var},
    OutOfMemory,
};
use rustsat_glucose::core::Glucose;
use std::collections::HashMap;
use thiserror::Error;

mod dimensions;
mod grid;
mod point;

fn main() -> anyhow::Result<()> {
    let mut instance: SatInstance<BasicVarManager> = SatInstance::new();
    let dims = Dimensions::new(10, 10);

    let point_map = build_clauses(&mut instance, dims).context("failed to build clauses")?;

    for max_cardinality in (1..=10).rev() {
        let mut solver = Glucose::default();

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

        let grid = Grid::from_map(dims, |p| {
            sol.var_value(*point_map.get(&p).unwrap())
                .to_bool_with_def(false)
        });

        let marked_count = grid.iter().filter(|&&x| x).count();
        println!("Solution: ({marked_count} marked)");

        print_grid(&grid, |b| {
            b.then_some(block_char::FULL)
                .unwrap_or(block_char::LIGHT_SHADE)
        });
    }

    Ok(())
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
    solver: &mut Glucose,
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
    dims: Dimensions,
) -> Result<HashMap<Point, Var>, OutOfMemory> {
    let mut point_lit_map = HashMap::new();

    let var_manager = instance.var_manager_mut();
    for p in dims.iter_within() {
        let var = var_manager.new_var();
        point_lit_map.insert(p, var);
    }

    for point in dims.iter_within() {
        let clause: Clause = Clause::from_iter(
            point
                .iter_within_manhattan(3)
                .filter(|p| dims.contains(*p))
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
}
