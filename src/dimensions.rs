use crate::point::{Point, PointTy};

pub type DimTy = u8;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Dimensions {
    pub width: DimTy,
    pub height: DimTy,
}

impl Dimensions {
    pub const fn new(width: DimTy, height: DimTy) -> Self {
        Dimensions { width, height }
    }

    pub const fn contains(self, point: Point) -> bool {
        point.x >= 0
            && point.x < self.width as PointTy
            && point.y >= 0
            && point.y < self.height as PointTy
    }

    /// Iterates points within this rectangle.
    /// For yielded points, `0 <= x < self.x` and `0 <= y < self.y`.
    pub const fn iter_within(self) -> DimensionsIter {
        DimensionsIter::new(self)
    }
}

/// Iterates exclusively - yielded values are never equal to the x or y of
/// `dims`
pub struct DimensionsIter {
    dims: Dimensions,
    current: Point,
}

impl DimensionsIter {
    pub const fn new(dims: Dimensions) -> Self {
        DimensionsIter { dims, current: Point::new(0, 0) }
    }
}

impl Iterator for DimensionsIter {
    type Item = Point;
    fn next(&mut self) -> Option<Self::Item> {
        // Note: iterates exclusively!
        if self.current.y >= self.dims.height as PointTy {
            return None;
        }
        let val = self.current;

        // Step x, step y and reset x if out of bounds
        self.current.x += 1;
        if self.current.x >= self.dims.width as PointTy {
            self.current.x = 0;
            self.current.y += 1;
        }

        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use assertables::{assert_all, assert_len_eq_x};

    use super::*;

    #[test]
    fn iter_dims() {
        let dims = Dimensions::new(7, 9);
        let points = dims.iter_within().collect::<Vec<_>>();

        assert_len_eq_x!(points, 7 * 9);
        assert_all!(points.iter(), |p: &Point| dims.contains(*p))
    }
}
