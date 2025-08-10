//! # Platform SAT encoding
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
    fmt::{Display, Formatter},
    hash::Hash,
};

use derive_more::From;
use enum_map::Enum;
use itertools::Itertools;
use petgraph::{graph::DiGraph, graphmap::NodeTrait, prelude::NodeIndex};

use crate::{
    dimensions::Dimensions,
    platform::{PLATFORMS_DEFAULT, PlatformDef},
    point::Point,
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

/// Maps dimensions to platform definitions, including rotated variants.
pub fn platform_dim_map(
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

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, iter, path::Path};

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
        let platform_map = platform_dim_map(&PLATFORMS_DEFAULT);
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

        let node_topo = petgraph::algo::toposort(&dag, None).expect("toposort failed");
        let (toposorted, revmap) = petgraph::algo::tred::dag_to_toposorted_adjacency_list::<
            _,
            NodeIndex,
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
}
