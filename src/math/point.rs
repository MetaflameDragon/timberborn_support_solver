use std::{
    fmt::{Display, Formatter},
    ops::{Add, Mul, Neg, Sub},
};

use rustsat::types::Lit;
use serde::{Deserialize, Serialize};

// The math is a mess but whatever

#[derive(Debug, Copy, Clone, Default)]
#[derive(PartialEq, Eq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct Point {
    pub x: isize,
    pub y: isize,
}

impl Display for Point {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl Point {
    pub const fn new(x: isize, y: isize) -> Self {
        Point { x, y }
    }

    pub const fn manhattan_mag(self) -> usize {
        self.x.unsigned_abs() + self.y.unsigned_abs()
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

    pub fn as_lit_pos(self) -> Option<Lit> {
        let upper = (self.x as u32) << 16;
        let lower = self.y as u32;
        Lit::new_with_error(upper | lower, false).ok()
    }

    /// Swaps x and y
    pub const fn flipped(self) -> Point {
        Point::new(self.y, self.x)
    }

    /// A conditional version of [`flipped`][`Self::flipped`]
    pub const fn flipped_if(self, cond: bool) -> Point {
        if cond { self.flipped() } else { self }
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

impl Mul<isize> for Point {
    type Output = Point;
    fn mul(self, rhs: isize) -> Point {
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
        IterManhattan { center, dist, iter_point_rel: Point::new(0, -(dist as isize)) }
    }
}

impl Iterator for IterManhattan {
    type Item = Point;
    fn next(&mut self) -> Option<Self::Item> {
        // Going from bottom to top
        // If past the top, the iterator is done
        if self.iter_point_rel.y > self.dist as isize {
            return None;
        }

        // Absolute position
        let val = self.iter_point_rel + self.center;

        // Step x, step y if that would go past the maximum distance
        self.iter_point_rel.x += 1;
        if self.iter_point_rel.manhattan_mag() > self.dist {
            self.iter_point_rel.y += 1;
            self.iter_point_rel.x = -(self.dist as isize - self.iter_point_rel.y.abs());
        }

        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use assertables::*;

    use super::*;

    #[test]
    fn iter_manhattan() {
        let c = Point { x: 1, y: 2 };
        let manhattan_points = c.iter_within_manhattan(3).collect::<Vec<_>>();

        assert_len_eq_x!(manhattan_points.clone(), 1 + 3 + 5 + 7 + 5 + 3 + 1);
        let order_predicate = |a: Point, b: Point| a.y < b.y || a.y == b.y && a.x < b.x;
        assert_all!(manhattan_points.windows(2), |pair: &[Point]| {
            order_predicate(pair[0], pair[1])
        });
    }
}
