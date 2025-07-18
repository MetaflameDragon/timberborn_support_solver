use std::ops::Div;

use crate::{
    dimensions::Dimensions,
    point::{Point, PointTy},
};

#[derive(Clone, Debug)]
pub struct Grid<T> {
    data: Vec<T>,
    dims: Dimensions,
}

impl<T> Grid<T> {
    pub const fn dims(&self) -> Dimensions {
        self.dims
    }

    pub fn iter_rows(&self) -> impl Iterator<Item = &[T]> {
        debug_assert_eq!(self.data.len() % self.dims.width as usize, 0);
        self.data.chunks_exact(self.dims.width as usize)
    }

    pub fn from_map(dims: Dimensions, map_fn: impl Fn(Point) -> T) -> Self {
        Grid { data: dims.iter_within().map(&map_fn).collect(), dims }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    pub fn enumerate(&self) -> impl Iterator<Item = (Point, &T)> {
        self.data.iter().enumerate().map(|(i, val)| (self.index_to_point(i), val))
    }

    pub fn get(&self, point: Point) -> Option<&T> {
        if !self.dims.contains(point) {
            return None;
        }
        let i = point.x as usize + (point.y as usize * self.dims.width as usize);
        Some(&self.data[i])
    }

    fn index_to_point(&self, index: usize) -> Point {
        Point::new(
            (index % self.dims.width as usize) as PointTy,
            index.div(self.dims.width as usize) as PointTy,
        )
    }

    pub fn try_from_vec(dims: Dimensions, data: Vec<T>) -> Option<Self> {
        if dims.width as usize * dims.height as usize != data.len() {
            return None;
        }
        Some(Grid { data, dims })
    }
}

impl<T: Default + Clone> Grid<T> {
    pub fn new(dims: Dimensions) -> Self {
        Self::new_fill(dims, T::default())
    }
}

impl<T: Clone> Grid<T> {
    pub fn new_fill(dims: Dimensions, value: T) -> Self {
        let Some(flat_size) = dims.width.checked_mul(dims.height) else {
            panic!("Dimensions too large! {}*{} would overflow", dims.width, dims.height);
        };
        Grid { data: vec![value; flat_size as usize], dims }
    }
}
