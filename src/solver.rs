use std::{array, collections::HashMap, fs::File, io::Write, iter::once};

use anyhow::{Context, bail, ensure};
use assertables::{assert_gt, assert_le};
use log::{error, info};
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, SatInstance},
    solvers::{Interrupt, Solve, SolverResult},
    types::{Assignment, Var, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use thiserror::Error;

use crate::{
    Command, InterrupterContainer, TERRAIN_SUPPORT_DISTANCE,
    dimensions::Dimensions,
    grid::Grid,
    load_grid_from_file,
    platform::{Platform, PlatformType},
    point::Point,
    world::World,
};

pub struct Solver<'a> {
    vars: Variables,
    instance: SatInstance,
    world: &'a World,
}

impl Solver<'_> {
    pub fn new(world: &'_ World) -> Solver<'_> {
        let mut instance: SatInstance<BasicVarManager> = SatInstance::new();

        let mut vars: Variables = Default::default();

        // Add vars for platforms, and implication clauses between them
        for p in world.terrain_grid().dims().iter_within() {
            let var_1 = instance.new_var();
            let var_3 = instance.new_var();
            let var_5 = instance.new_var();
            vars.platforms_1x1.insert(p, var_1);
            vars.platforms_3x3.insert(p, var_3);
            vars.platforms_5x5.insert(p, var_5);

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
        for (&p, &var_5x5) in vars.platforms_5x5.iter() {
            // 5x5 <-> 1x1
            for x in p.x..=(p.x + 4) {
                for y in p.y..=(p.y + 4) {
                    let p_other = Point::new(x, y);
                    if p == p_other {
                        continue;
                    }
                    let Some(var_1x1) = vars.platforms_1x1.get(&p_other).copied() else { continue };

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
                    let Some(var_3x3) = vars.platforms_3x3.get(&p_other).copied() else { continue };

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
                    let Some(var_5x5_b) = vars.platforms_5x5.get(&p_other).copied() else {
                        continue;
                    };

                    instance.add_lit_impl_lit(var_5x5.pos_lit(), var_5x5_b.neg_lit());
                }
            }
        }

        // 3x3 <-> _
        for (&p, &var_3x3) in vars.platforms_3x3.iter() {
            // 3x3 <-> 1x1
            for x in p.x..=(p.x + 2) {
                for y in p.y..=(p.y + 2) {
                    let p_other = Point::new(x, y);
                    if p == p_other {
                        continue;
                    }
                    let Some(var_1x1) = vars.platforms_1x1.get(&p_other).copied() else { continue };

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
                    let Some(var_3x3_b) = vars.platforms_3x3.get(&p_other).copied() else {
                        continue;
                    };

                    instance.add_lit_impl_lit(var_3x3.pos_lit(), var_3x3_b.neg_lit());
                }
            }
        }

        // Terrain support goal clauses
        // Terrain support distance is handled in "layers", like variables stacked on
        // top of one another
        // Tiles from each layer imply their neighbors in the next
        // All tiles in the topmost layer are goals
        // All tiles in the bottom-most layer are implied by
        // the platforms supporting them

        // New vars for all occupied spaces in the terrain grid
        for p in world.terrain_grid().enumerate().filter_map(|(p, &v)| v.then_some(p)) {
            vars.terrain_layers.insert(p, array::from_fn(|_| instance.new_var()));
        }

        for (p, layers) in &vars.terrain_layers {
            // Layer implication clauses
            for lower_i in 0..(TERRAIN_SUPPORT_DISTANCE - 1) {
                let upper_i = lower_i + 1;

                // Upper tile -> disjunction of tiles below (direct & neighbors)
                // Equiv: conjunction of ~tiles below -> ~upper tile

                let upper_tile = layers[upper_i].pos_lit();
                let lower_tiles = p
                    .neighbors()
                    .iter()
                    .chain(once(p))
                    .filter_map(|q| vars.terrain_layers.get(q))
                    .map(|col| col[lower_i].pos_lit())
                    .collect::<Vec<_>>();

                instance.add_lit_impl_clause(upper_tile, &lower_tiles);
            }

            // Require the topmost layer
            instance.add_unit(layers.last().unwrap().pos_lit());

            // Lowest tile -> disjunction of all platforms below
            let mut platforms: Vec<Var> = Vec::new();

            platforms.extend(vars.platforms_1x1.get(&p));
            for x in (p.x - 2)..=p.x {
                for y in (p.y - 2)..=p.y {
                    let q = Point::new(x, y);
                    platforms.extend(vars.platforms_3x3.get(&q));
                }
            }
            for x in (p.x - 4)..=p.x {
                for y in (p.y - 4)..=p.y {
                    let q = Point::new(x, y);
                    platforms.extend(vars.platforms_5x5.get(&q));
                }
            }

            instance.add_lit_impl_clause(
                layers.first().unwrap().pos_lit(),
                &platforms.iter().map(|v| v.pos_lit()).collect::<Vec<_>>(),
            );
        }

        // instance.convert_to_cnf(); // Might not be needed?

        Solver { vars, instance, world }
    }

    pub fn solve(
        &mut self,
        interrupter: InterrupterContainer,
        max_cardinality: usize,
    ) -> Result<Option<Solution>, SolveError> {
        let mut sat_solver = GlucoseSimp::default();
        *interrupter.lock().expect("Mutex was poisoned!") =
            Some(Box::new(sat_solver.interrupter()));

        info!(target: "solver", "Solving for n <= {max_cardinality}...");

        let upper_constraint = CardConstraint::new_ub(
            self.vars.platforms_1x1.values().map(|var| var.pos_lit()),
            max_cardinality,
        );

        let assignment = match try_solve(&mut sat_solver, self.instance.clone(), upper_constraint) {
            Ok(sol) => sol,
            Err(SolveError::Unsat) => {
                info!(target: "solver", "Unsat");
                return Ok(None);
            }
            Err(SolveError::Interrupted) => {
                info!(target: "solver", "Interrupted");
                return Err(SolveError::Interrupted);
            }
            Err(SolveError::Other(err)) => {
                info!(target: "solver", "Error: {err:?}");
                return Err(SolveError::Other(err));
            }
        };

        let mut sol: Solution = Solution::from_assignment(&assignment, &self.vars, self.world);
        if let Err(err) = sol.validate() {
            error!(target: "solver", "Error validating solution: {err:?}");

            panic!(
                r"The solution unexpectedly failed to validate!
                  This is a bug and should not occur.
                  Details:
                  {err}"
            );
        };

        info!(target: "solver", "Solution found ({} marked)", sol.platform_count());

        Ok(Some(sol))
    }
}

#[derive(Clone, Debug)]
pub struct Solution<'a> {
    platforms: HashMap<Point, PlatformType>,
    world: &'a World,
}

impl Solution<'_> {
    pub fn from_assignment<'a>(
        assignment: &Assignment,
        variables: &Variables,
        world: &'a World,
    ) -> Solution<'a> {
        let mut platforms = HashMap::new();

        for (&p, &var) in &variables.platforms_1x1 {
            if assignment.var_value(var).to_bool_with_def(false) {
                platforms.insert(p, PlatformType::Square1x1);
            }
        }
        for (&p, &var) in &variables.platforms_3x3 {
            if assignment.var_value(var).to_bool_with_def(false) {
                platforms.insert(p, PlatformType::Square3x3);
            }
        }
        for (&p, &var) in &variables.platforms_5x5 {
            if assignment.var_value(var).to_bool_with_def(false) {
                platforms.insert(p, PlatformType::Square5x5);
            }
        }

        Solution { platforms, world }
    }

    pub fn platform_count(&self) -> usize {
        self.platforms.iter().count()
    }

    pub fn get_platform(&self, p: Point) -> Option<Platform> {
        self.platforms.get(&p).map(|t| Platform::new(p, *t))
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // Sanity check: a world with non-empty terrain must have at least one platform
        if self.platform_count() == 0 && self.world.terrain_grid().iter().all(|x| !x) {
            bail!("The solution contains 0 platforms for a non-empty world grid");
        }

        // Platform overlap
        // TODO: should this collect all overlaps? only a few? or create a log file?
        let overlapping_platforms: Vec<_> = self
            .platforms
            .iter()
            .enumerate()
            .flat_map(|(i, (p1, t1))| {
                self.platforms.iter().enumerate().filter_map(move |(j, (p2, t2))| {
                    // Check only one triangle of the cross product
                    (i < j).then_some({
                        let plat_a = Platform::new(*p1, *t1);
                        let plat_b = Platform::new(*p2, *t2);
                        (plat_a, plat_b)
                    })
                })
            })
            .filter(|(plat_a, plat_b)| plat_a.overlaps(plat_b))
            .collect();
        if !overlapping_platforms.is_empty() {
            for (plat_a, plat_b) in &overlapping_platforms {
                error!(target: "validation", "Overlap! {:?} <-> {:?}", plat_a, plat_b);
            }
            bail!(
                "The solution contains overlapping platforms ({} overlaps found)",
                overlapping_platforms.len()
            );
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Variables {
    platforms_1x1: HashMap<Point, Var>,
    platforms_3x3: HashMap<Point, Var>,
    platforms_5x5: HashMap<Point, Var>,
    terrain_layers: HashMap<Point, [Var; TERRAIN_SUPPORT_DISTANCE]>,
}

impl Variables {
    pub fn write_var_map<W: Write>(&self, w: &mut W) -> std::io::Result<()> {
        for (var, repr) in self.to_var_repr_map() {
            writeln!(w, "{var} {repr}")?;
        }

        Ok(())
    }

    pub fn to_var_repr_map(&self) -> HashMap<Var, String> {
        let mut map = HashMap::new();

        for (prefix, hashmap) in
            [("P1", &self.platforms_1x1), ("P3", &self.platforms_3x3), ("P5", &self.platforms_5x5)]
        {
            for (point, &var) in hashmap {
                let Point { x, y } = point;
                map.insert(var, format!("{prefix}({x},{y})"));
            }
        }

        for (point, layers) in &self.terrain_layers {
            for (i, &var) in layers.iter().enumerate() {
                let Point { x, y } = point;
                map.insert(var, format!("T{i}({x},{y})"));
            }
        }

        map
    }
}

#[derive(Error, Debug)]
pub enum SolveError {
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
