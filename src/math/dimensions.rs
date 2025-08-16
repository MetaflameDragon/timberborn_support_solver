use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
};

use serde::{Deserialize, Serialize};

use crate::math::Point;

/// 2D dimensions with a width and a height.
///
/// Dimensions have a partial ordering defined,
/// such that: `a <= b` <-> `a` is contained within `b`.
#[derive(Debug, Copy, Clone, Eq, Default)]
#[derive(Serialize, Deserialize)]
pub struct Dimensions {
    pub width: usize,
    pub height: usize,
}

impl Dimensions {
    pub const fn new(width: usize, height: usize) -> Self {
        Dimensions { width, height }
    }

    pub const fn width(self) -> usize {
        self.width
    }

    pub const fn height(self) -> usize {
        self.height
    }

    pub const fn flipped(self) -> Dimensions {
        Dimensions { width: self.height, height: self.width }
    }

    /// Returns the corner point within these dimensions, or none if empty or
    /// outside of usize.
    pub fn corner_point_incl(self) -> Option<Point> {
        let (x, y) = (self.width().checked_sub(1)?, self.height().checked_sub(1)?);
        (x <= isize::MAX as usize && y <= isize::MAX as usize)
            .then_some(Point::new(x as isize, y as isize))
    }

    pub const fn contains(self, point: Point) -> bool {
        point.x >= 0
            && point.x < self.width as isize
            && point.y >= 0
            && point.y < self.height as isize
    }

    pub const fn contains_dims(self, other: Dimensions) -> bool {
        other.empty() || self.width >= other.width && self.height >= other.height
    }

    /// Iterates points within this rectangle.
    /// For yielded points, `0 <= x < self.width` and `0 <= y < self.height`.
    pub const fn iter_within(self) -> DimensionsIter {
        DimensionsIter::new(self)
    }

    pub const fn empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl PartialEq for Dimensions {
    fn eq(&self, other: &Self) -> bool {
        self.empty() && other.empty() || self.width == other.width && self.height == other.height
    }
}

impl PartialOrd for Dimensions {
    /// `a <= b` <-> `a` is contained within `b`.
    ///
    /// Two dimensions with the same width and height are equal.
    /// If one is lesser on at least one axis, it is lesser, vice versa for
    /// greater.
    ///
    /// If one axis is lesser but the other is greater, there is no ordering.
    ///
    /// An empty dimension ([`Self::empty()`]) is always contained within any
    /// other dimension. If both are empty, they're equal.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use Ordering::*;
        // Cases:
        // Both empty => equal
        // One empty => empty is less
        // xy equal => equal
        // x less but y greater or opposite => none
        // one of xy less or greater, not both equal, not different
        // => less or greater
        Some(match (self.empty(), other.empty()) {
            // Exactly one is empty
            (true, false) => Less,
            (false, true) => Greater,
            // Both empty
            (true, true) => Equal,
            // Size comparison:
            (false, false) => {
                match (self.width.cmp(&other.width), self.height.cmp(&other.height)) {
                    // Equal
                    (Equal, Equal) => Equal,
                    // xy compare differently
                    (Less, Greater) | (Greater, Less) => None?,
                    // Nonequal, comparable, at least one is less or greater
                    (Less, _) | (_, Less) => Less,
                    (Greater, _) | (_, Greater) => Greater,
                }
            }
        })
    }
}

impl Hash for Dimensions {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if !self.empty() {
            self.width.hash(state);
            self.height.hash(state);
        }
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
    use assertables::{assert_all, assert_len_eq_x};

    use super::*;

    #[test]
    fn iter_dims() {
        let dims = Dimensions::new(7, 9);
        let points = dims.iter_within().collect::<Vec<_>>();

        assert_len_eq_x!(points.clone(), 7 * 9);
        assert_all!(points.iter(), |p: &Point| dims.contains(*p))
    }
}
