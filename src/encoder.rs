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
    fmt::{Display, Formatter},
    hash::RandomState,
};

use derive_more::From;
use enum_map::Enum;
use itertools::iproduct;
use petgraph::graphmap::DiGraphMap;

use crate::{
    dimensions::{DimTy, Dimensions},
    platform::PlatformType,
    point::Point,
};

#[derive(Copy, Clone, Debug)]
#[derive(Eq, PartialEq, Hash)]
#[derive(From)]
pub enum Node {
    Platform(PlatformType),
    Terrain(Point),
}

/// Node needs Ord & PartialOrd so it can be used in DiGraphMap
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Node::Terrain(a), Node::Terrain(b)) => a.cmp(b),
            (Node::Terrain(_), Node::Platform(_)) => Ordering::Less,
            (Node::Platform(_), Node::Terrain(_)) => Ordering::Greater,
            (Node::Platform(a), Node::Platform(b)) => a.into_usize().cmp(&b.into_usize()),
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
            Node::Platform(plat) => write!(f, "Platform {plat}"),
            Node::Terrain(Point { x, y }) => write!(f, "({x}, {y})"),
        }
    }
}

/// Creates a directed acyclic graph for platforms and terrain tiles.
///
/// For platforms `a` and `b`, `a <- b` iff `area(a) <: area(b)`, i.e. `b`
/// entirely encapsulates `a`.
///
/// The graph is generated at runtime, so caching is recommended. It is also
/// transitively closed. See also
/// [`dag_transitive_reduction_closure`][`petgraph::algo::tred::dag_transitive_reduction_closure`].
pub fn platform_type_graph() -> DiGraphMap<Node, (), RandomState> {
    // Note: RustRover handles conditionally disable type params incorrectly, so the
    // default has to be specified explicitly
    let mut g = DiGraphMap::<Node, (), _>::new();

    for (plat, other) in
        iproduct!(enum_iterator::all::<PlatformType>(), enum_iterator::all::<PlatformType>())
    {
        // Skip self loops! This makes it easier to use toposort later
        if plat == other {
            continue;
        }

        let self_corner = plat.area_outer_corner_relative();
        let other_corner = other.area_outer_corner_relative();
        if self_corner.x >= other_corner.x && self_corner.y >= other_corner.y {
            // larger (self) -> smaller (other)
            g.add_edge(plat.into(), other.into(), ());
        }
    }

    for plat in enum_iterator::all::<PlatformType>() {
        let self_corner = plat.area_outer_corner_relative();
        for point in
            Dimensions::new(self_corner.x as DimTy + 1, self_corner.y as DimTy + 1).iter_within()
        {
            // platform -> terrain
            // Note that the implication in the SAT encoding will be reverse!
            // More specifically: terrain -> (plat1 | plat2 | ...) for all in-edges
            g.add_edge(plat.into(), point.into(), ());
        }
    }

    g
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path};

    use petgraph::{
        EdgeType, Graph,
        adj::DefaultIx,
        dot::{Config, Dot},
        graph::{IndexType, NodeIndex},
    };

    use super::*;

    #[test]
    fn platform_type_graph_print() {
        let g = platform_type_graph();

        use petgraph::visit::NodeRef;

        let mut dag = g.into_graph::<DefaultIx>();
        write_graph(&dag, "platform_graph.dot");

        let node_topo = petgraph::algo::toposort(&dag, None).expect("toposort failed");
        let (toposorted, revmap) = petgraph::algo::tred::dag_to_toposorted_adjacency_list::<
            _,
            NodeIndex,
        >(&dag, &node_topo);

        let (graph_reduced, graph_closure) =
            petgraph::algo::tred::dag_transitive_reduction_closure(&toposorted);

        dbg!(&toposorted);
        dbg!(&graph_reduced);

        dag.retain_edges(|g, e| {
            let (a, b) = g.edge_endpoints(e).unwrap(); // Should not fail - we just got e
            graph_reduced.contains_edge(revmap[a.index()], revmap[b.index()])
        });
        write_graph(&dag, "platform_graph_reduced.dot");

        fn write_graph<E, Ty, Ix, P: AsRef<Path>>(g: &Graph<Node, E, Ty, Ix>, path: P)
        where
            Ty: EdgeType,
            Ix: IndexType,
        {
            let mut file = fs::File::options()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .expect("Failed to create/overwrite file.");
            let string_g = g.map(|_: NodeIndex<Ix>, n| n, |_, _| String::new());
            let dot = Dot::with_attr_getters(
                &string_g,
                &[Config::EdgeNoLabel],
                &|_, _| String::new(),
                &|_, n| {
                    match n.weight() {
                        Node::Platform(_) => "group = \"platform\"",
                        Node::Terrain(_) => "group = \"terrain\"",
                    }
                    .to_string()
                },
            );

            write!(file, "{}", dot).expect("Failed to write to file.");
        }
    }
}
