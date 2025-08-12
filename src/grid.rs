use std::ops::Div;

use serde::{Deserialize, Serialize};

use crate::{dimensions::Dimensions, point::Point};

#[derive(Clone, Debug, Serialize, Deserialize)]
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

    pub fn from_fn<F>(dims: Dimensions, map_fn: F) -> Self
    where
        F: FnMut(Point) -> T,
    {
        Grid { data: dims.iter_within().map(map_fn).collect(), dims }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    pub fn enumerate(&self) -> impl Iterator<Item = (Point, &T)> {
        self.data.iter().enumerate().map(|(i, val)| (self.index_to_point(i), val))
    }

    pub fn get(&self, point: Point) -> Option<&T> {
        self.data_index(point).map(|i| &self.data[i])
    }

    pub fn get_mut(&mut self, point: Point) -> Option<&mut T> {
        self.data_index(point).map(|i| &mut self.data[i])
    }

    pub fn set(&mut self, point: Point, mut item: T) -> Option<T> {
        let i = self.data_index(point)?;
        std::mem::swap(&mut self.data[i], &mut item);
        Some(item)
    }

    fn data_index(&self, point: Point) -> Option<usize> {
        self.dims.contains(point).then(|| point.x as usize + (point.y as usize * self.dims.width))
    }

    fn index_to_point(&self, index: usize) -> Point {
        Point::new((index % self.dims.width) as isize, index.div(self.dims.width) as isize)
    }

    pub fn try_from_vec(dims: Dimensions, data: Vec<T>) -> Option<Self> {
        if dims.width * dims.height != data.len() {
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
