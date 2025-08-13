//! # Platform SAT encoding
//!
//! TODO: Rewrite/update
//!
//! There are various sizes of platforms, both rectangular and square-shaped. In
//! particular, Timberborn has the basic 1x1 platform, then larger 3x3 and 5x5
//! variants, and then rectangular ones from 1x2 to 1x6 (with rotated variants).
//!
//! A naive encoding would specify one variable per tile for every platform
//! type, and set a constraint that at most one of these may be set to true.
//! However, since larger platforms "extend" the support area of smaller
//! variants, we can view them as a hierarchy, edges lead from larger platforms
//! to smaller ones that can fully fit within their area.
//!
//! In particular, platform types form a directed acyclic graph, such that, for
//! platforms `a`, `b`, there's an edge `a -> b` iff `a`'s area encompasses `b`.
//! These edges are then treated as implication constraints for the SAT
//! encoding (`a` implies `b`).
//!
//! ## Solution soundness
//!
//! For the goal of supporting terrain tiles, it would be unsound to select two
//! types of platforms such that their combined set of supported tiles is not
//! satisfiable by any one of them alone. In the game itself, platforms may not
//! overlap, so there cannot be two platforms in one tile. When a platform
//! cannot be selected from the set of active platform variables such that its
//! set of supported tiles would be the superset of all the others, the solution
//! would be unsound.
//!
//! The naive encoding ensures this soundness by limiting selection to (at most)
//! one platform type per tile. Simply put, the constraints make it impossible
//! for a solution to have two platform variables active within a tile.
//!
//! A linear chain of implications from larger to smaller platforms (as
//! described above) also remains sound, because the supported tiles of a larger
//! platform are a superset of the smaller platform. With multiple platform
//! variables active per tile, we can then choose the largest platform from the
//! set.
//!
//! The set of platform types doesn't form a linear chain, however. In a linear
//! implication chain, a larger platform implies all smaller platforms, so
//! choosing the largest platform is not an issue. However, when there are
//! multiple branches, we run into the same issue as described above.
//!
//! ```text
//!   /-- 1x2 <- 1x3 <- 1x4 <- 1x5 <- 1x6
//!  v            ^             ^
//! 1x1          3x3 <-------- 5x5
//!  ^            v             v
//!   \-- 2x1 <- 3x1 <- 4x1 <- 5x1 <- 6x1
//! ```
//!
//! This is the full transitively reduced graph of all platform types.
//! A directed edge `a -> b` - also an implication in the encoding - means that
//! the set of tiles supported by platform `a` is a superset of that of
//! platform `b`. Since this is a directed acyclic graph, there is a (strict)
//! partial ordering between the nodes, where we can look for maximal and
//! minimal elements.
//!
//! Selecting the largest platform (as described above) equates to choosing the
//! maximal element. In order for the solution to be sound, there must be
//! exactly one maximal element in the set of active platform variables.
//!
//! To encode this requirement using boolean constraints, we'll start by finding
//! every pair of nodes that do not have an ordering. These nodes must not be
//! active at the same time, unless a node that is greater than both is also
//! active.
//!
//! Let `a, b, c, d, e, f` be nodes, and let the graph be the union of
//! `a <- b <- d <- f` and `a <- c <- e <- f`.
//! `a <- {b, c}` and `{b, c} <- ... <- f`, so the resulting clause is
//! `(~b | ~c | f)`.
//!
//! In the previous example, none of `b`, `c`, `d`, `e` are comparable, but,
//! thanks to the implication clauses (`b <- d`, `c <- e`), we only need to add
//! a constraint for `b` and `c`. Similarly, if there is a node `g` greater than
//! `b`, `c`, but also `f`, we only need to add `f` to the clause, since
//! `f <- g`.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    env::var,
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    iter,
    marker::PhantomData,
};

use derive_more::From;
use enum_map::Enum;
use itertools::Itertools;
use petgraph::{
    adj::{DefaultIx, IndexType, UnweightedList},
    graph::DiGraph,
    graphmap::NodeTrait,
    prelude::{NodeIndex, *},
    visit::{IntoEdgeReferences, IntoEdges, IntoNodeIdentifiers, IntoNodeReferences},
};
use rustsat::{
    instances::{BasicVarManager, Cnf, ManageVars, OptInstance, SatInstance},
    types::{Clause, Lit, Var},
};

use crate::{
    TERRAIN_SUPPORT_DISTANCE,
    dimensions::Dimensions,
    grid::Grid,
    platform::{PLATFORMS_DEFAULT, Platform, PlatformDef},
    point::Point,
    world::WorldGrid,
};

#[derive(Copy, Clone, Debug)]
#[derive(Eq, PartialEq, Hash)]
#[derive(From)]
pub enum Node {
    Platform(Dimensions),
    Terrain(Point),
}

/// Node needs Ord & PartialOrd so it can be used in DiGraphMap
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Node::Terrain(a), Node::Terrain(b)) => a.cmp(b),
            (Node::Terrain(_), Node::Platform(_)) => Ordering::Less,
            (Node::Platform(_), Node::Terrain(_)) => Ordering::Greater,
            (Node::Platform(a), Node::Platform(b)) => {
                // Ordering specific to Node! Does not match dimension ordering
                (a.width(), a.height()).cmp(&(b.width(), b.height()))
            }
        }
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Mainly for printing the Dot format
impl Display for Node {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Platform(dims) => write!(f, "Platform {}x{}", dims.width(), dims.height()),
            Node::Terrain(Point { x, y }) => write!(f, "({x}, {y})"),
        }
    }
}

// Might be useful to have a typed index? delete if not needed maybe
struct TypedIx<T, Ix = DefaultIx>(Ix, PhantomData<T>);

impl<T, Ix: Clone> Clone for TypedIx<T, Ix> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1)
    }
}
impl<T, Ix: Copy> Copy for TypedIx<T, Ix> {}
impl<T, Ix: Default> Default for TypedIx<T, Ix> {
    fn default() -> Self {
        Self(Ix::default(), Default::default())
    }
}
impl<T, Ix: Hash> Hash for TypedIx<T, Ix> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<T, Ix: Ord> Ord for TypedIx<T, Ix> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T, Ix: PartialOrd> PartialOrd for TypedIx<T, Ix> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl<T, Ix: PartialEq> PartialEq for TypedIx<T, Ix> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}
impl<T, Ix: Eq> Eq for TypedIx<T, Ix> {}
impl<T, Ix: Debug> Debug for TypedIx<T, Ix> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TypedIx<{}>({:?})", std::any::type_name::<T>(), self.0)
    }
}

unsafe impl<Ix: IndexType, T: 'static> IndexType for TypedIx<T, Ix> {
    fn new(x: usize) -> Self {
        Self(Ix::new(x), Default::default())
    }

    fn index(&self) -> usize {
        self.0.index()
    }

    fn max() -> Self {
        Self(<Ix as IndexType>::max(), Default::default())
    }
}

/// Maps dimensions to platform definitions, including rotated variants.
pub fn dims_platform_map(
    platform_defs: &[PlatformDef],
) -> HashMap<Dimensions, HashSet<PlatformDef>> {
    let mut map: HashMap<_, HashSet<PlatformDef>> = HashMap::new();
    for p in platform_defs.iter() {
        map.entry(p.dims()).or_default().insert(*p);
        map.entry(p.dims().flipped()).or_default().insert(*p);
    }
    map
}

/// Creates a directed acyclic graph for a set of items with [`PartialOrd`],
/// such that smaller -> larger.
///
/// Uses a wrapper node type that must implement `Ord`.
pub fn dag_by_partial_ord<T>(items: &[T]) -> DiGraph<T, (), usize>
where
    T: PartialOrd + Clone + Eq + Hash,
{
    let mut g = DiGraph::<T, (), usize>::with_capacity(items.len(), items.len().saturating_sub(1));

    let mut item_map = HashMap::new();

    for item in items {
        item_map.insert(item, g.add_node(item.clone()));
    }

    for (this, other) in items.iter().cartesian_product(items) {
        if other < this {
            // smaller (other) -> larger (this)
            g.add_edge(item_map[other], item_map[this], ());
        }
    }

    g
}

#[derive(Clone, Debug)]
struct EncodingTileVars {
    dims_vars: HashMap<Dimensions, Var>,
    terrain: Option<[Var; TERRAIN_SUPPORT_DISTANCE]>,
}

impl EncodingTileVars {
    pub fn for_dims(&self, dims: Dimensions) -> Option<Var> {
        self.dims_vars.get(&dims).cloned()
    }
}

#[derive(Clone, Debug)]
enum EncodedItem {
    Platform { point: Point, dims: Dimensions },
    Terrain { point: Point, layer: usize },
}

#[derive(Clone, Debug)]
pub struct EncodingVars {
    dim_map: HashMap<Dimensions, HashSet<PlatformDef>>,
    grid: Grid<EncodingTileVars>,
    var_map: HashMap<Var, EncodedItem>,
}

impl EncodingVars {
    pub fn new(
        platform_defs: &[PlatformDef],
        terrain: &WorldGrid,
        var_man: &mut BasicVarManager,
    ) -> Self {
        let dim_map = dims_platform_map(platform_defs);

        let dim_keys: Vec<_> = dim_map.keys().cloned().collect();
        let grid = Grid::from_fn(terrain.dims(), |p| EncodingTileVars {
            dims_vars: dim_keys.iter().cloned().map(|k| (k, var_man.new_var())).collect(),
            terrain: terrain.get(p).unwrap().then_some(std::array::from_fn(|_| var_man.new_var())),
        });
        let mut var_map = HashMap::new();
        for (point, vars) in grid.enumerate() {
            for (&dims, &var) in vars.dims_vars.iter() {
                var_map.insert(var, EncodedItem::Platform { point, dims });
            }
            for (layer, var) in vars.terrain.iter().flatten().enumerate() {
                var_map.insert(*var, EncodedItem::Terrain { point, layer });
            }
        }
        Self { dim_map, grid, var_map }
    }

    pub fn at(&self, point: Point) -> Option<&EncodingTileVars> {
        self.grid.get(point)
    }
    pub fn for_dims_at(&self, point: Point, dims: Dimensions) -> Option<Var> {
        self.grid.get(point)?.for_dims(dims)
    }
    pub fn platform_dims(&self) -> impl Iterator<Item = Dimensions> + Clone {
        self.dim_map.keys().cloned().into_iter()
    }

    pub fn iter_dims_vars(&self, dims: Dimensions) -> Option<impl Iterator<Item = Var>> {
        self.dim_map
            .contains_key(&dims)
            .then_some(self.grid.iter().map(move |vars| vars.dims_vars[&dims]))
    }

    pub fn var_map(&self) -> &HashMap<Var, EncodedItem> {
        &self.var_map
    }

    pub fn dims_platform_map(&self) -> &HashMap<Dimensions, HashSet<PlatformDef>> {
        &self.dim_map
    }

    pub fn var_to_platform(&self, var: Var) -> Option<Platform> {
        self.var_map.get(&var).and_then(|item| {
            if let EncodedItem::Platform { point, dims } = item {
                let def = self
                    .dim_map
                    .get(dims)
                    .and_then(|s| s.iter().next())
                    .expect("encoded platform did not map to a platform def");

                debug_assert!(def.dims() == *dims || def.dims() == dims.flipped());
                let rotated = def.dims() != *dims;

                Some(Platform::new(*point, *def, rotated))
            } else {
                None
            }
        })
    }

    pub fn lit_readable_name(&self, lit: Lit) -> Option<String> {
        self.var_map.get(&lit.var()).map(|item| match item {
            EncodedItem::Platform { point, dims } => {
                format!(
                    "{}P{}x{}({};{})",
                    if lit.is_neg() { "~" } else { "" },
                    dims.width,
                    dims.height,
                    point.x,
                    point.y
                )
            }
            EncodedItem::Terrain { point, layer } => {
                format!(
                    "{}T{}({};{})",
                    if lit.is_neg() { "~" } else { "" },
                    layer,
                    point.x,
                    point.y
                )
            }
        })
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum EncodingNode {
    Platform(Dimensions),
    Point(Point),
}

impl PartialOrd for EncodingNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (EncodingNode::Platform(dims), EncodingNode::Platform(other)) => {
                dims.partial_cmp(other)
            }
            (EncodingNode::Platform(dims), EncodingNode::Point(point)) => {
                dims.contains(*point).then_some(Ordering::Greater)
            }
            (EncodingNode::Point(point), EncodingNode::Platform(dims)) => {
                dims.contains(*point).then_some(Ordering::Less)
            }
            (EncodingNode::Point(a), EncodingNode::Point(b)) => a.eq(b).then_some(Ordering::Equal),
        }
    }
}

enum Toposorted {}

#[derive(Clone, Debug)]
struct EncodingDag {
    dag: DiGraph<EncodingNode, (), usize>,
    closure: UnweightedList<TypedIx<Toposorted>>,
    reduced: UnweightedList<TypedIx<Toposorted>>,
    revmap: Vec<TypedIx<Toposorted>>,
    topo: Vec<NodeIndex<usize>>,
}

impl EncodingDag {
    pub fn new(platform_dims: impl Iterator<Item = Dimensions> + Clone) -> Self {
        let dims_max = platform_dims.clone().fold(Dimensions::new(1, 1), |acc, dims| {
            Dimensions::new(acc.width.max(dims.width), acc.height.max(dims.height))
        });

        let mut dag = dag_by_partial_ord(
            &platform_dims
                .map(EncodingNode::Platform)
                .chain(dims_max.iter_within().map(EncodingNode::Point))
                .collect_vec(),
        );

        // All points in the maximal rectangle covering all dimensions are tested
        // Some point nodes may end up without any edges to platform dimensions
        dag.retain_nodes(|g, n| g.neighbors_undirected(n).next().is_some());

        // Guide:
        // dag -> topo -> toposorted & revmap -> graph_reduced & graph_closure
        // dag[topo[graph_reduced index]]
        // graph_reduced[revmap[dag index]]
        let topo = petgraph::algo::toposort(&dag, None).expect("toposort failed");
        let (dims_toposorted, revmap) = petgraph::algo::tred::dag_to_toposorted_adjacency_list::<
            _,
            TypedIx<Toposorted>,
        >(&dag, &topo);

        let (reduced, closure) =
            petgraph::algo::tred::dag_transitive_reduction_closure(&dims_toposorted);

        Self { dag, reduced, closure, revmap, topo }
    }

    pub fn iter_edges_reduced(&self) -> impl Iterator<Item = (EncodingNode, EncodingNode)> {
        self.reduced.edge_references().map(|e| {
            (self.dag[self.topo[e.source().index()]], self.dag[self.topo[e.target().index()]])
        })
    }

    pub fn iter_platform_edges_reduced(&self) -> impl Iterator<Item = (Dimensions, Dimensions)> {
        self.iter_edges_reduced().filter_map(|pair| match pair {
            (EncodingNode::Platform(source), EncodingNode::Platform(target)) => {
                Some((source, target))
            }
            _ => None,
        })
    }

    pub fn iter_point_platform_edges_reduced(&self) -> impl Iterator<Item = (Point, Dimensions)> {
        self.iter_edges_reduced().filter_map(|pair| match pair {
            (EncodingNode::Point(source), EncodingNode::Platform(target)) => Some((source, target)),
            _ => None,
        })
    }

    pub fn iter_platform_targets_by_source(
        &self,
    ) -> impl Iterator<Item = Vec<(TypedIx<Toposorted>, Dimensions)>> {
        self.dag
            .node_references()
            .filter_map(|(ix, n)| match n {
                EncodingNode::Platform(dims) => Some((ix, *dims)),
                _ => None,
            })
            .map(|(ix, dims)| {
                self.reduced
                    .edges(self.revmap[ix.index()])
                    .map(|e| e.target())
                    .map(|ix| match self.dag[self.topo[ix.index()]] {
                        EncodingNode::Platform(dims) => (ix, dims),
                        _ => {
                            unreachable!(
                                "out-edges from platform nodes are expected to always be platforms"
                            )
                        }
                    })
                    .collect::<Vec<_>>()
            })
    }

    pub fn common_platform_successors(
        &self,
        a: TypedIx<Toposorted>,
        b: TypedIx<Toposorted>,
    ) -> Vec<(TypedIx<Toposorted>, Dimensions)> {
        self.closure
            .edges(a)
            .filter_map(|e| {
                if self.closure.contains_edge(b, e.target())
                    && let EncodingNode::Platform(dims) = self.dag[self.topo[e.target().index()]]
                {
                    Some((e.target(), dims))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn maximal_from(&self, indices: &[TypedIx<Toposorted>]) -> Vec<TypedIx<Toposorted>> {
        indices
            .into_iter()
            .copied()
            .filter(|n| !indices.iter().any(|m| self.closure.contains_edge(*m, *n)))
            .collect()
    }
}

pub fn encode(
    platform_defs: &[PlatformDef],
    terrain: &WorldGrid,
    instance: &mut SatInstance<BasicVarManager>,
) -> EncodingVars {
    // TODO: A lot of places here rely on all tiles having all platform vars
    // maybe this should expect those lookups to be fallible?

    let vars = EncodingVars::new(platform_defs, terrain, instance.var_manager_mut());

    let dag = EncodingDag::new(vars.platform_dims());
    dbg!(&dag);

    for p in terrain.dims().iter_within() {
        let current_vars = vars.at(p).unwrap();

        // ===== Platform selection DAG =====
        for (smaller, larger) in dag.iter_platform_edges_reduced() {
            // The DAG has nodes ordered as smaller -> larger
            // This means that 1x1 has no in-edges, and the largest platforms have no
            // out-edges We want the implications encoded as smaller <- larger
            instance.add_lit_impl_lit(
                current_vars.for_dims(larger).unwrap().pos_lit(),
                current_vars.for_dims(smaller).unwrap().pos_lit(),
            );
        }

        for ((ix_a, dims_a), (ix_b, dims_b)) in dag
            .iter_platform_targets_by_source()
            .flat_map(|targets| Itertools::tuple_combinations(targets.into_iter()))
        {
            // All nodes `n` such that `a ->+ n` and `b ->+ n`
            // Then filters out nodes `n` for `m ->* n`
            let common_successors = dag.common_platform_successors(ix_a, ix_b);
            let common_successors_maximal_ixs: Vec<_> =
                dag.maximal_from(&common_successors.iter().map(|(ix, _)| *ix).collect_vec());
            let common_successors_maximal = common_successors.iter().filter_map(|(ix, dims)| {
                common_successors_maximal_ixs.contains(ix).then_some(*dims)
            });
            // Result: pairs `a`, `b` and an associated set C, where:
            // `a ->+ c in C`, `b ->+ c in C`, `c, d in C: c !->+ d`
            // `->+`: transitive successor (1 or more edges)
            // `!->+`: not a transitive successor

            // (~a | ~b | i1 | i2...), or also (a & b) -> (i1 | i2...)
            instance.add_cube_impl_clause(
                &[
                    current_vars.for_dims(dims_a).unwrap().pos_lit(),
                    current_vars.for_dims(dims_b).unwrap().pos_lit(),
                ],
                &common_successors_maximal
                    .into_iter()
                    .map(|dims| current_vars.for_dims(dims).unwrap().pos_lit())
                    .collect_vec(),
            );
        }

        // ===== Platform-terrain clauses =====
        // Point -> disjunction of platforms
        // For the current tile, look at all platforms that support this tile.
        // (This means platforms to the top-left of the current point.)
        // This is effectively a reverse iteration - rather than taking a platform
        // _here_ and binding _other_ tiles to it, this takes the _current_ tile and
        // binds _other_ platforms to it. The point doesn't move, where we look for
        // other platforms moves.
        // First make sure there even _is_ a terrain tile above
        if let Some(point_var) = current_vars.terrain.map(|t| t[TERRAIN_SUPPORT_DISTANCE - 1]) {
            let platform_vars =
                dag.iter_point_platform_edges_reduced().filter_map(|(offset, dims)| {
                    // Check that we're not looking out of bounds, and get the platform var there
                    vars.at(p - offset).map(|v| v.dims_vars[&dims])
                });

            // If this binds to no platforms (shouldn't at the moment!), it'll become p ->
            // [] An empty clause is unsat, so p would be forced to be false (as
            // expected) It also translates to a [~p] CNF clause, so yeah,
            // simple unit neg literal
            instance.add_lit_impl_clause(
                point_var.pos_lit(),
                &platform_vars.map(|v| v.pos_lit()).collect_vec(),
            );
        }

        // ===== Terrain support =====

        if let Some(point_terrain) = current_vars.terrain {
            // For the current tile, get all neighbors
            let neighbor_terrain_vars: Vec<_> = p
                .neighbors()
                .into_iter()
                .flat_map(|n| vars.at(n).and_then(|v| v.terrain))
                .chain(iter::once(point_terrain))
                .collect();
            // Add for all layers, i -> j for i + 1 = j
            for i in 0..(TERRAIN_SUPPORT_DISTANCE - 1) {
                let j = i + 1;
                // heights: i -> j
                // Starts at 0 going down
                // TERRAIN_SUPPORT_DISTANCE-1 is closest to platforms

                // Add an implication: tile -> disjunction of neighbors below
                instance.add_lit_impl_clause(
                    point_terrain[i].pos_lit(),
                    &neighbor_terrain_vars.iter().map(|v| v[j].pos_lit()).collect_vec(),
                );
            }

            // Finally, require the topmost var
            // TODO: this can be optimized trivially
            instance.add_unit(point_terrain[0].pos_lit());
        }

        // ===== Platform overlap =====

        // TODO
    }

    vars
}

#[cfg(test)]
mod tests {
    use std::{fmt::Debug, fs, hash::Hasher, io::Write, iter, marker::PhantomData, path::Path};

    use itertools::Itertools;
    use petgraph::{
        EdgeType, Graph,
        adj::DefaultIx,
        dot::{Config, Config::NodeNoLabel, Dot},
        graph::{IndexType, NodeIndex},
        prelude::*,
        visit::{IntoEdges, IntoNodeIdentifiers, IntoNodeReferences},
    };

    use super::*;

    #[test]
    fn platform_type_graph_print() {
        let platform_map = dims_platform_map(&PLATFORMS_DEFAULT);
        let g = dag_by_partial_ord(&platform_map.keys().cloned().collect::<Vec<Dimensions>>());

        use petgraph::visit::NodeRef;

        let getter = |dim: Dimensions| {
            format!(
                "{}x{}\\n({})",
                dim.width(),
                dim.height(),
                platform_map.get(&dim).unwrap().iter().join(", ")
            )
        };

        let dag = g.map(|_, dim| Node::Platform(*dim), |_, e| *e);
        write_graph(&dag, "platform_graph.dot", getter);

        // Guide:
        // dag -> node_topo -> toposorted & revmap -> graph_reduced & graph_closure
        // node_topo: graph_reduced index -> dag index
        // revmap: dag index -> graph_reduced index

        enum Toposorted {};
        let node_topo = petgraph::algo::toposort(&dag, None).expect("toposort failed");
        let (toposorted, revmap) = petgraph::algo::tred::dag_to_toposorted_adjacency_list::<
            _,
            TypedIx<Toposorted>,
        >(&dag, &node_topo);

        let (graph_reduced, graph_closure) =
            petgraph::algo::tred::dag_transitive_reduction_closure(&toposorted);

        dbg!(&dag);
        dbg!(&node_topo);
        dbg!(&toposorted);
        dbg!(&revmap);
        dbg!(&graph_reduced);

        let mut platform_graph_reduced = dag.clone();
        platform_graph_reduced.retain_edges(|g, e| {
            let (a, b) = g.edge_endpoints(e).unwrap(); // Should not fail - we just got e
            graph_reduced.contains_edge(revmap[a.index()], revmap[b.index()])
        });
        write_graph(&platform_graph_reduced, "platform_graph_reduced.dot", getter);

        fn node_is_platform<Ix: IndexType>(
            g: &Graph<Node, (), Directed, Ix>,
            i: NodeIndex<Ix>,
        ) -> bool {
            let node = (*g)[i];
            matches!(node, Node::Platform(_))
        }

        for (a, b) in graph_reduced.node_identifiers().flat_map(|i| {
            graph_reduced
                .edges(i)
                .map(|e| e.target())
                .combinations(2)
                .map(|combo| (combo[0], combo[1]))
        }) {
            // All nodes `n` such that `a ->+ n` and `b ->+ n`
            let common_successors: Vec<_> = graph_closure
                .edges(a)
                .filter_map(|e| graph_closure.contains_edge(b, e.target()).then_some(e.target()))
                .collect();
            // Filters out nodes `n` for `m ->* n`
            let common_successors_maximal: Vec<_> = common_successors
                .iter()
                .filter(|n| !common_successors.iter().any(|m| graph_closure.contains_edge(*m, **n)))
                .collect();
            // Result: pairs `a`, `b` and an associated set C, where:
            // `a ->+ c in C`, `b ->+ c in C`, `c, d in C: c !->+ d`
            // `->+`: transitive successor (1 or more edges)
            // `!->+`: not a transitive successor
            let node_a = platform_graph_reduced[node_topo[a.index()]];
            let node_b = platform_graph_reduced[node_topo[b.index()]];
            let nodes_successors = common_successors_maximal
                .into_iter()
                .map(|i| platform_graph_reduced[node_topo[i.index()]]);
            println!("!{} | !{} | {}", node_a, node_b, nodes_successors.into_iter().join(" | "));
        }

        fn write_graph<E, Ty, Ix, P: AsRef<Path>, F: Fn(Dimensions) -> String>(
            g: &Graph<Node, E, Ty, Ix>,
            path: P,
            dim_label_getter: F,
        ) where
            Ty: EdgeType,
            Ix: IndexType,
        {
            let mut file = fs::File::options()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .expect("Failed to create/overwrite file.");
            let string_g = g.map(|_: NodeIndex<Ix>, n| *n, |_, _| String::new());
            let attr_getter = |_, n: (NodeIndex<Ix>, &Node)| match n.weight() {
                Node::Platform(dim) => {
                    format!("label = \"{}\" group = \"platform\"", dim_label_getter(*dim))
                        .to_string()
                }
                Node::Terrain(_) => "group = \"terrain\"".to_string(),
            };
            let dot = Dot::with_attr_getters(
                &string_g,
                &[Config::EdgeNoLabel, NodeNoLabel],
                &|_, _| String::new(),
                &attr_getter,
            );

            write!(file, "{}", dot).expect("Failed to write to file.");
        }
    }

    #[test]
    fn encode_test() {
        let grid = b"\
            .--.\
            .-..\
            ....\
            --..\
              "
        .map(|c| match c as char {
            '.' => true,
            _ => false,
        })
        .to_vec();
        let terrain = WorldGrid(Grid::try_from_vec(Dimensions::new(4, 4), grid).unwrap());

        let mut instance = SatInstance::<BasicVarManager>::new();
        let vars = encode(&PLATFORMS_DEFAULT, &terrain, &mut instance);

        dbg!(&vars);
        dbg!(&instance);

        for clause in instance.cnf().iter() {
            let clause_str = clause
                .iter()
                .map(|&l| vars.lit_readable_name(l).unwrap_or(format!("{l:?}")))
                .join(" | ");
            println!("{}", clause_str);
        }
    }
}
