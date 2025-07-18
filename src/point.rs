use std::{
    collections::{HashSet, VecDeque},
    fmt::{Display, Formatter},
    ops::{Add, Mul, Neg, Sub},
};

use rustsat::types::Lit;

use crate::grid::Grid;
// The math is a mess but whatever

pub type PointTy = isize;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Default)]
pub struct Point {
    pub x: PointTy,
    pub y: PointTy,
}

impl Display for Point {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl Point {
    pub const fn new(x: PointTy, y: PointTy) -> Self {
        Point { x, y }
    }

    pub const fn manhattan_mag(self) -> usize {
        self.x.unsigned_abs() as usize + self.y.unsigned_abs() as usize
    }

    pub const fn abs(self) -> Self {
        Point { x: self.x.abs(), y: self.y.abs() }
    }

    pub fn manhattan_to(self, other: Point) -> usize {
        (self - other).manhattan_mag()
    }

    pub const fn iter_within_manhattan(self, dist: usize) -> IterManhattan {
        IterManhattan::new(self, dist)
    }

    pub const fn neighbors(self) -> [Point; 4] {
        [
            Point::new(self.x + 1, self.y),
            Point::new(self.x, self.y + 1),
            Point::new(self.x - 1, self.y),
            Point::new(self.x, self.y - 1),
        ]
    }

    /// Returns a set of points up to `depth` away, which were reached through
    /// adjacent connections based on `grid`.
    pub fn adjacent_points(self, depth: usize, grid: &Grid<bool>) -> HashSet<Point> {
        let mut points = HashSet::new();

        let mut queue = VecDeque::new();
        queue.push_back((self, 0));
        points.insert(self);

        while let Some((point, p_depth)) = queue.pop_front() {
            for n in point.neighbors() {
                // Skip if grid has false (or out of bounds)
                if grid.get(n) != Some(&true) {
                    continue;
                }
                // Try to add, skip enqueue if already added
                if !points.insert(n) {
                    continue;
                }
                // Don't enqueue if too deep
                if p_depth + 1 >= depth {
                    continue;
                }
                queue.push_back((n, p_depth + 1));
            }
        }

        points
    }

    pub fn as_lit_pos(self) -> Option<Lit> {
        let upper = (self.x as u32) << 16;
        let lower = self.y as u32;
        Lit::new_with_error(upper | lower, false).ok()
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Neg for Point {
    type Output = Point;
    fn neg(self) -> Point {
        Point::new(-self.x, -self.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, rhs: Point) -> Point {
        self + (-rhs)
    }
}

impl Mul<PointTy> for Point {
    type Output = Point;
    fn mul(self, rhs: PointTy) -> Point {
        Point::new(self.x * rhs, self.y * rhs)
    }
}

pub struct IterManhattan {
    center: Point,
    dist: usize,
    iter_point_rel: Point,
}

impl IterManhattan {
    pub const fn new(center: Point, dist: usize) -> Self {
        IterManhattan { center, dist, iter_point_rel: Point::new(0, -(dist as PointTy)) }
    }
}

impl Iterator for IterManhattan {
    type Item = Point;
    fn next(&mut self) -> Option<Self::Item> {
        // Going from bottom to top
        // If past the top, the iterator is done
        if self.iter_point_rel.y > self.dist as PointTy {
            return None;
        }

        // Absolute position
        let val = self.iter_point_rel + self.center;

        // Step x, step y if that would go past the maximum distance
        self.iter_point_rel.x += 1;
        if self.iter_point_rel.manhattan_mag() > self.dist {
            self.iter_point_rel.y += 1;
            self.iter_point_rel.x = -(self.dist as PointTy - self.iter_point_rel.y.abs());
        }

        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use assertables::*;

    use super::*;
    use crate::dimensions::Dimensions;

    #[test]
    fn iter_manhattan() {
        let c = Point { x: 1, y: 2 };
        let manhattan_points = c.iter_within_manhattan(3).collect::<Vec<_>>();

        assert_len_eq_x!(manhattan_points, 1 + 3 + 5 + 7 + 5 + 3 + 1);
        let order_predicate = |a: Point, b: Point| a.y < b.y || a.y == b.y && a.x < b.x;
        assert_all!(manhattan_points.windows(2), |pair: &[Point]| {
            order_predicate(pair[0], pair[1])
        });
    }

    #[test]
    fn adjacent_points() {
        let c = Point { x: 2, y: 2 };
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
        let grid = Grid::try_from_vec(Dimensions::new(4, 4), grid).unwrap();

        // .  .
        // . ..
        // ..X.
        //   ..
        // ->
        // .  3
        // 3 12
        // 2101
        //   12

        let adjacent_points = c.adjacent_points(3, &grid);

        assert_set_eq!(
            adjacent_points,
            [(0, 1), (0, 2), (1, 2), (2, 1), (2, 2), (2, 3), (3, 0), (3, 1), (3, 2), (3, 3)]
                .iter()
                .map(|t| Point::new(t.0, t.1))
                .collect::<Vec<_>>()
        );
    }
}
