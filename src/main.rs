use dimensions::Dimensions;
use grid::Grid;
use rustsat::{
    encodings::card::{BoundUpper, Totalizer},
    instances::{BasicVarManager, ObjectVarManager, SatInstance},
    solvers::{Solve, SolverResult},
    types::Clause,
};
use rustsat_glucose::core::Glucose;
use std::{
    collections::HashMap,
    ops::{Add, Mul, Neg, Sub},
};

mod dimensions;
mod grid;
mod point;

fn main() {
    let mut instance: SatInstance<ObjectVarManager> = SatInstance::new();

    let grid: Grid<bool> = Grid::new(Dimensions::new(10, 10));

    let dims = grid.dims();

    let mut point_map = HashMap::new();

    let var_manager = instance.var_manager_mut();
    for p in dims.iter_within() {
        let var = var_manager.object_var(p);
        point_map.insert(var, p);
    }

    for point in dims.iter_within() {
        let var_manager = instance.var_manager_mut();
        let clause: Clause = Clause::from_iter(
            point
                .iter_within_manhattan(3)
                .filter(|p| dims.contains(*p))
                .map(|p| var_manager.object_var(p).pos_lit()),
        );
        instance.add_clause(clause);
    }

    let mut solver = Glucose::default();

    let (cnf, mut var_manager) = instance.into_cnf();
    solver.add_cnf(cnf).expect("Failed to add clause");

    let max_cardinality = 10;
    rustsat::encodings::card::new_default_ub()
        .encode_ub(..=max_cardinality, &mut solver, &mut var_manager)
        .expect("Failed to encode cardinality");

    let res = solver.solve();

    match res {
        Ok(SolverResult::Sat) => {
            println!("SAT");

            let sol = solver.full_solution();

            match sol {
                Ok(sol) => {
                    let positive_lits: Vec<_> = sol.iter().filter(|lit| lit.is_pos()).collect();
                    println!("Solution: ({} positive)", positive_lits.len());

                    for lit in positive_lits {
                        let var = lit.var();
                        let point = point_map.get(&var).unwrap();
                        println!("{} ({})", point, lit);
                    }
                }
                Err(err) => {
                    print!("Error: {:?}", err);
                }
            }
        }
        Ok(SolverResult::Unsat) => {
            println!("UNSAT");
            return;
        }
        Ok(SolverResult::Interrupted) => {
            println!("INTERRUPTED");
            return;
        }
        Err(err) => {
            println!("Error while solving");
            println!("{err}");
            return;
        }
    };
}
