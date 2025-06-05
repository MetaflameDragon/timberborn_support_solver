use dimensions::Dimensions;
use grid::Grid;
use rustsat::{
    instances::SatInstance,
    solvers::Solve,
    types::{Clause, Lit},
};
use rustsat_glucose::core::Glucose;
use std::ops::{Add, Mul, Neg, Sub};
use rustsat::instances::BasicVarManager;

mod dimensions;
mod grid;
mod point;

fn main() {
    let mut instance: SatInstance<BasicVarManager> = SatInstance::new();

    let grid: Grid<bool> = Grid::new(Dimensions::new(10, 10));

    for point in grid.dims().iter_within() {
        let clause: Clause = Clause::from_iter(
            point
                .iter_within_manhattan(3)
                .map(|p| p.as_lit_pos().expect("Failed to convert point")),
        );
        instance.add_clause(clause);
    }

    let mut solver = Glucose::default();

    let (cnf, var_manager) = instance.into_cnf();
    solver.add_cnf(cnf).expect("Failed to add clause");
}
