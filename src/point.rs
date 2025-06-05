use rustsat::types::Lit;
use std::{
    fmt::{Display, Formatter},
    ops::{Add, Mul, Neg, Sub},
};
// The math is a mess but whatever

pub type PointTy = i16;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
        Point {
            x: self.x.abs(),
            y: self.y.abs(),
        }
    }

    pub fn manhattan_to(self, other: Point) -> usize {
        (self - other).manhattan_mag()
    }

    pub const fn iter_within_manhattan(self, dist: usize) -> IterManhattan {
        IterManhattan::new(self, dist)
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
        IterManhattan {
            center,
            dist,
            iter_point_rel: Point::new(0, -(dist as PointTy)),
        }
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
    use super::*;
    use assertables::{assert_all, assert_len_eq_x};

    #[test]
    fn iter_manhattan() {
        let c = Point { x: 1, y: 2 };
        let manhattan_points = c.iter_within_manhattan(3).collect::<Vec<_>>();

        assert_len_eq_x!(manhattan_points, 1 + 3 + 5 + 7 + 5 + 3 + 1);
        let order_predicate = |a: Point, b: Point| a.y < b.y || a.y == b.y && a.x < b.x;
        assert_all!(manhattan_points.windows(2), |pair: &[Point]| {
            order_predicate(*&pair[0], *&pair[1])
        });
    }
}
