use rustsat::instances::SatInstance;
use rustsat_glucose::core::Glucose;
use std::ops::{Add, Mul, Neg, Sub};

fn main() {
    let mut instance = SatInstance::new();
    let mut solver = Glucose::default();
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
