use std::{
    array,
    collections::HashMap,
    io::Write,
    iter::once,
    num::NonZero,
    ops::{Add, AddAssign},
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use anyhow::{Context, anyhow};
use derive_more::{Deref, DerefMut};
use enum_iterator::Sequence;
use enum_map::EnumMap;
use futures::{FutureExt, SinkExt, Stream, StreamExt, TryFutureExt};
use log::info;
use new_zealand::nz;
use rustsat::{
    encodings::{card, card::Totalizer},
    instances::{BasicVarManager, SatInstance},
    solvers::{Interrupt, InterruptSolver, Solve},
    types::{Assignment, Var, constraints::CardConstraint},
};
use rustsat_glucose::simp::Glucose as GlucoseSimp;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    encoder::EncodingVars,
    platform::{PLATFORMS_DEFAULT, Platform, PlatformDef},
    point::Point,
    world::World,
};

pub mod dimensions;
pub mod encoder;
pub mod grid;
pub mod platform;
pub mod point;
pub mod utils;
pub mod world;

const TERRAIN_SUPPORT_DISTANCE: usize = 4;

pub struct SolverConfig {
    vars: EncodingVars,
    instance: SatInstance,
}

#[derive(Debug, Clone)]
pub struct SolverRunConfig {
    pub limits: PlatformLimits,
}

impl SolverRunConfig {
    pub fn max_platforms(&self) -> Option<usize> {
        todo!()
        // self.limits.get(&PlatformDef::Square1x1).copied()
    }

    pub fn limits(&self) -> &PlatformLimits {
        &self.limits
    }

    pub fn limits_mut(&mut self) -> &mut PlatformLimits {
        &mut self.limits
    }
}

#[derive(Clone, Debug, Deref, DerefMut)]
pub struct PlatformLimits(HashMap<PlatformDef, usize>);

impl PlatformLimits {
    pub fn new(map: HashMap<PlatformDef, usize>) -> Self {
        Self(map)
    }
}

pub enum SolverResponse {
    /// A solution from a valid assignment
    Sat(Solution),
    /// No solution found for the current problem
    Unsat,
    /// The solver session was aborted by the user
    Aborted,
}

impl SolverConfig {
    pub fn new(world: &World) -> SolverConfig {
        let mut instance: SatInstance<BasicVarManager> = SatInstance::new();
        let vars = encoder::encode(&PLATFORMS_DEFAULT, world.grid(), &mut instance);

        SolverConfig { vars, instance }
    }

    pub fn vars(&self) -> &EncodingVars {
        &self.vars
    }

    pub fn instance(&self) -> &SatInstance {
        &self.instance
    }

    pub fn start(&self, cfg: &SolverRunConfig) -> anyhow::Result<(SolverFuture, Interrupter)> {
        let mut sat_solver = GlucoseSimp::default();
        let vars = self.vars.clone();
        let (cnf, mut var_manager) = self.instance.clone().into_cnf();
        sat_solver.add_cnf(cnf).context("Failed to add CNF")?;

        for (&platform_type, &limit) in cfg.limits().iter() {
            // TODO: Actually encode limits for other platforms
            // TODO: Clarify what the encoding means
            // The cardinality constraints still work in terms of "platform
            // promotion", where smaller platforms "promote" to
            // larger ones, and the vars for larger ones imply all
            // their predecessors. This means that limiting 5x5 to
            // <= 2 and 3x3 to <= 4 actually means "at most 4 of 3x3
            // or larger, and at most 2 of 5x5". info!("Limiting {}
            // platforms to n <= {}", platform_type, limit);
            // let upper_constraint = CardConstraint::new_ub(
            //     vars.platform_vars_map(platform_type).values().map(|var|
            // var.pos_lit()),     limit,
            // );
            //
            // card::encode_cardinality_constraint::<Totalizer, _>(
            //     upper_constraint,
            //     &mut sat_solver,
            //     &mut var_manager,
            // )
            // .context("failed to encode cardinality constraint")?;
        }

        // TODO: Temporary 1x1-only limiter
        if let (platform_type, Some(&limit)) =
            (PLATFORMS_DEFAULT[0], cfg.limits().get(&PLATFORMS_DEFAULT[0]))
        {
            info!("Limiting {} platforms to n <= {}", platform_type, limit);
            let upper_constraint = CardConstraint::new_ub(
                vars.iter_dims_vars(platform_type.dims()).unwrap().map(|var| var.pos_lit()),
                limit,
            );

            card::encode_cardinality_constraint::<Totalizer, _>(
                upper_constraint,
                &mut sat_solver,
                &mut var_manager,
            )
            .context("failed to encode cardinality constraint")?;
        }

        let interrupter = Box::new(sat_solver.interrupter());

        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<SolverResponse> {
            use rustsat::solvers::SolverResult as SatSolverResult;
            let result = match sat_solver.solve()? {
                SatSolverResult::Sat => SolverResponse::Sat(Solution::from_assignment(
                    &sat_solver.full_solution()?,
                    &vars,
                )),
                SatSolverResult::Unsat => SolverResponse::Unsat,
                SatSolverResult::Interrupted => SolverResponse::Aborted,
            };
            Ok(result)
        });

        Ok((SolverFuture { handle }, interrupter))
    }
}

pub type Interrupter = Box<dyn InterruptSolver + Send>;

pub struct SolverFuture {
    handle: tokio::task::JoinHandle<anyhow::Result<SolverResponse>>,
}

impl SolverFuture {
    pub fn handle(&self) -> &tokio::task::JoinHandle<anyhow::Result<SolverResponse>> {
        &self.handle
    }

    pub fn future(self) -> impl Future<Output = anyhow::Result<SolverResponse>> {
        self.handle.unwrap_or_else(|join_err| Err(anyhow!(join_err)))
    }
}

// TODO maybe eventually write a proper IntoFuture
// impl IntoFuture for SolverFuture {
//     type Output = anyhow::Result<SolverResponse>;
//     type IntoFuture = _;
//
//     fn into_future(self) -> Self::IntoFuture {
//         core::future::IntoFuture::into_future(self.future())
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub world: World, // TODO: run configs/profiles, previous sessions, etc.
}

#[derive(Clone, Debug, Default)]
pub struct Variables {
    platforms_1x1: HashMap<Point, Var>,
    platforms_3x3: HashMap<Point, Var>,
    platforms_5x5: HashMap<Point, Var>,
    terrain_layers: HashMap<Point, [Var; TERRAIN_SUPPORT_DISTANCE]>,
}

impl Variables {
    /// A map of all variables for a specific platform type
    pub fn platform_vars_map(&self, ty: PlatformDef) -> &HashMap<Point, Var> {
        todo!()
        //
        // match ty {
        //     Square1x1 => &self.platforms_1x1,
        //     Square3x3 => &self.platforms_3x3,
        //     Square5x5 => &self.platforms_5x5,
        //     Rect1x2(_) => todo!(),
        // }
    }

    /// Variable for a single platform
    pub fn platform_var(&self, ty: PlatformDef, point: Point) -> Option<Var> {
        self.platform_vars_map(ty).get(&point).cloned()
    }

    /// Variable for a single terrain tile
    pub fn terrain_var(&self, point: Point) -> Option<Var> {
        self.terrain_layers.get(&point).map(|layer| layer[TERRAIN_SUPPORT_DISTANCE - 1])
    }

    /// All variables for terrain tiles
    pub fn terrain_vars(&self) -> impl Iterator<Item = (Point, Var)> {
        self.terrain_layers.iter().map(|(&p, l)| (p, l[TERRAIN_SUPPORT_DISTANCE - 1]))
    }

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

fn encode_world_constraints(
    world: &World,
    instance: &mut SatInstance<BasicVarManager>,
) -> Variables {
    let terrain_grid = world.grid();

    let mut vars: Variables = Default::default();

    // Add vars for platforms, and implication clauses between them
    for p in terrain_grid.dims().iter_within() {
        // let mut map: EnumMap<PlatformDef, Option<Var>> = EnumMap::default();
        // for plat in enum_iterator::all::<PlatformDef>() {
        //     map[plat] = Some(instance.new_var()); // TODO
        // }
        todo!();

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
    for p in terrain_grid.enumerate().filter_map(|(p, &v)| v.then_some(p)) {
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

        platforms.extend(vars.platforms_1x1.get(p));
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

    vars
}

#[derive(Clone, Debug)]
pub struct Solution {
    platforms: HashMap<Point, Platform>,
}

impl Solution {
    pub fn from_assignment(assignment: &Assignment, vars: &EncodingVars) -> Self {
        let mut platforms = HashMap::new();

        // iter() goes over all assigned literals (excl. DontCare)
        for (lit, plat) in assignment
            .iter()
            .filter_map(|lit| Some((lit, lit.is_pos().then(|| vars.var_to_platform(lit.var()))??)))
        {
            info!("Active literal: {}", vars.lit_readable_name(lit).unwrap_or(format!("{lit:?}")));
            platforms
                .entry(plat.point())
                .and_modify(|previous: &mut Platform| {
                    if previous.def().dims() < plat.def().dims() {
                        // Update only if larger
                        *previous = plat;
                    }
                })
                .or_insert(plat);
            info!("=> {plat:?}");
        }

        // for (&p, &var) in &variables.platforms_1x1 {
        //     if assignment.var_value(var).to_bool_with_def(false) {
        //         platforms.insert(p, PlatformDef::Square1x1);
        //     }
        // }
        // for (&p, &var) in &variables.platforms_3x3 {
        //     if assignment.var_value(var).to_bool_with_def(false) {
        //         platforms.insert(p, PlatformDef::Square3x3);
        //     }
        // }
        // for (&p, &var) in &variables.platforms_5x5 {
        //     if assignment.var_value(var).to_bool_with_def(false) {
        //         platforms.insert(p, PlatformDef::Square5x5);
        //     }
        // }

        Solution { platforms }
    }

    pub fn platforms(&self) -> &HashMap<Point, Platform> {
        &self.platforms
    }

    pub fn platform_count(&self) -> usize {
        self.platforms.len()
    }

    /// Counts the number of occurrences of all platform types and returns them
    /// as a HashMap.
    ///
    /// Platform types with 0 occurrences do not appear in the map, as indicated
    /// by the NonZero value type.
    pub fn platform_stats(&self) -> HashMap<PlatformDef, NonZero<usize>> {
        /// Since this is used only for this one folding operation, the addition
        /// simply panics on overflow, since we assume that there are no more
        /// than usize platforms for any given type.
        fn increment(n: &mut NonZero<usize>) {
            *n = n.checked_add(1).unwrap();
        }

        self.platforms().iter().fold(HashMap::new(), |mut map, (_, &plat)| {
            map.entry(plat.def()).and_modify(increment).or_insert(nz!(1));
            map
        })
    }

    pub fn get_platform(&self, p: Point) -> Option<Platform> {
        self.platforms.get(&p).copied()
    }
}
