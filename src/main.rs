use rustsat::instances::SatInstance;
use rustsat_glucose::core::Glucose;
use std::ops::{Add, Mul, Neg, Sub};

fn main() {
    // let mut instance = SatInstance::new();

    // let grid: Grid<bool> = Grid::new(Dimensions::new(10, 10));

    // let mut solver = Glucose::default();
}

// The math is a mess but whatever

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Point {
    pub x: isize,
    pub y: isize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Dimensions {
    pub width: usize,
    pub height: usize,
}

struct IterManhattan {
    center: Point,
    dist: usize,
    iter_point_rel: Point,
}

impl IterManhattan {
    pub const fn new(center: Point, dist: usize) -> Self {
        IterManhattan {
            center,
            dist,
            iter_point_rel: Point::new(0, -(dist as isize)),
        }
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

impl Dimensions {
    pub const fn new(width: usize, height: usize) -> Self {
        Dimensions { width, height }
    }

    pub const fn contains(self, point: Point) -> bool {
        point.x >= 0
            && point.x < self.width as isize
            && point.y >= 0
            && point.y < self.height as isize
    }

    /// Iterates points within this rectangle.
    /// For yielded points, `0 <= x < self.x` and `0 <= y < self.y`.
    pub const fn iter_within(self) -> DimensionsIter {
        DimensionsIter::new(self)
    }
}

/// Iterates exclusively - yielded values are never equal to the x or y of
/// `dims`
struct DimensionsIter {
    dims: Dimensions,
    current: Point,
}

impl DimensionsIter {
    pub const fn new(dims: Dimensions) -> Self {
        DimensionsIter {
            dims,
            current: Point::new(0, 0),
        }
    }
}

impl Iterator for DimensionsIter {
    type Item = Point;
    fn next(&mut self) -> Option<Self::Item> {
        // Note: iterates exclusively!
        if self.current.y >= self.dims.height as isize {
            return None;
        }
        let val = self.current;

        // Step x, step y and reset x if out of bounds
        self.current.x += 1;
        if self.current.x >= self.dims.width as isize {
            self.current.x = 0;
            self.current.y += 1;
        }

        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Dimensions, Point};
    use assertables::{assert_all, assert_len_eq_x};
    use std::slice::Windows;

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

    #[test]
    fn iter_dims() {
        let dims = Dimensions::new(7, 9);
        let points = dims.iter_within().collect::<Vec<_>>();

        assert_len_eq_x!(points, 7 * 9);
        assert_all!(points.iter(), |p: &Point| dims.contains(*p))
    }
}

impl Point {
    pub const fn new(x: isize, y: isize) -> Self {
        Point { x, y }
    }

    pub const fn manhattan_mag(self) -> usize {
        self.x.abs() as usize + self.y.abs() as usize
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

impl Mul<usize> for Point {
    type Output = Point;
    fn mul(self, rhs: usize) -> Point {
        Point::new(
            (self.x as usize * rhs) as isize,
            (self.y as usize * rhs) as isize,
        )
    }
}

struct Grid<T> {
    data: Vec<T>,
    dims: Dimensions,
}

impl<T: Default + Clone> Grid<T> {
    pub fn new(dims: Dimensions) -> Self {
        let Some(flat_size) = dims.width.checked_mul(dims.height) else {
            panic!(
                "Dimensions too large! {}*{} would overflow",
                dims.width, dims.height
            );
        };
        Grid {
            data: vec![Default::default(); flat_size],
            dims,
        }
    }

    pub fn get(&self, point: Point) -> Option<&T> {
        if !self.dims.contains(point) {
            return None;
        }
        let i = point.x as usize + point.y as usize * self.dims.width;
        Some(&self.data[i])
    }
}
