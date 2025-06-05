use crate::{dimensions::Dimensions, point::Point};

pub struct Grid<T> {
    data: Vec<T>,
    dims: Dimensions,
}

impl<T> Grid<T> {
    pub const fn dims(&self) -> Dimensions {
        self.dims
    }
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
            data: vec![Default::default(); flat_size as usize],
            dims,
        }
    }

    pub fn get(&self, point: Point) -> Option<&T> {
        if !self.dims.contains(point) {
            return None;
        }
        let i = point.x as usize + (point.y as usize * self.dims.width as usize);
        Some(&self.data[i])
    }
}
