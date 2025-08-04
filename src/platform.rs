use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::point::Point;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PlatformType {
    Square1x1,
    Square3x3,
    Square5x5,
}

impl PlatformType {
    /// Two corners of the area this platform covers (relative to the origin).
    ///
    /// Both points are inclusive, and `0.x <= 1.x && 0.y <= 1.y`.
    pub fn area_corners_relative(self) -> (Point, Point) {
        match self {
            PlatformType::Square1x1 => (Point::new(0, 0), Point::new(0, 0)),
            PlatformType::Square3x3 => (Point::new(0, 0), Point::new(2, 2)),
            PlatformType::Square5x5 => (Point::new(0, 0), Point::new(4, 4)),
        }
    }

    pub const fn dimensions_str(self) -> &'static str {
        match self {
            PlatformType::Square1x1 => "1x1",
            PlatformType::Square3x3 => "3x3",
            PlatformType::Square5x5 => "5x5",
        }
    }
}

impl Display for PlatformType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dimensions_str())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Platform {
    point: Point,
    r#type: PlatformType,
}

impl Platform {
    pub fn new(point: Point, r#type: PlatformType) -> Self {
        Self { point, r#type }
    }

    /// Two corners of the area this platform covers (relative to this
    /// platform's position).
    ///
    /// Both points are inclusive, and `0.x <= 1.x && 0.y <= 1.y`.
    ///
    /// This is better than referring to a platform's inner `point` directly,
    /// since the point may be placed arbitrarily.
    pub fn area_corners(&self) -> (Point, Point) {
        let (a, b) = self.r#type.area_corners_relative();
        (a + self.point, b + self.point)
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        let (self_near, self_far) = self.area_corners();
        let (other_near, other_far) = other.area_corners();

        other_far.x >= self_near.x
            && other_far.y >= self_near.y
            && other_near.x <= self_far.x
            && other_near.y <= self_far.y
    }

    pub fn platform_type(&self) -> PlatformType {
        self.r#type
    }
}

macro_rules! platform {
    (1x1 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), PlatformType::Square1x1)
    };
    (3x3 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), PlatformType::Square3x3)
    };
    (5x5 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), PlatformType::Square5x5)
    };
}

#[cfg(test)]
mod tests {
    //noinspection RsUnusedImport
    use test_case::{test_case, test_matrix};

    use super::*;

    #[test_case(platform!(1x1 @ 2, 3), platform!(1x1 @ 2, 3))]
    #[test_case(platform!(1x1 @ 3, 3), platform!(1x1 @ 3, 3))]
    #[test_matrix(
        [platform!(3x3 @ 5, 5)],
        [
            platform!(1x1 @ 5, 5), platform!(1x1 @ 6, 6), platform!(1x1 @ 7, 7), platform!(1x1 @ 7, 5),
            platform!(3x3 @ 5, 5), platform!(3x3 @ 3, 3), platform!(3x3 @ 3, 7), platform!(3x3 @ 7, 7),
        ]
    )]
    #[test_matrix(
        [platform!(5x5 @ 5, 5)],
        [
            platform!(1x1 @ 5, 5), platform!(1x1 @ 6, 6), platform!(1x1 @ 9, 9), platform!(1x1 @ 9, 5),
            platform!(3x3 @ 5, 5), platform!(3x3 @ 3, 3), platform!(3x3 @ 3, 9), platform!(3x3 @ 9, 9),
            platform!(5x5 @ 5, 5), platform!(5x5 @ 1, 1), platform!(5x5 @ 1, 7), platform!(5x5 @ 9, 9),
        ]
    )]
    fn platform_overlap_yes(a: Platform, b: Platform) {
        assert!(
            a.overlaps(&b),
            r"
                Platforms SHOULD overlap
                Platform corners:
                {:?}
                {:?}
                ",
            a.area_corners(),
            b.area_corners()
        );
        assert!(
            b.overlaps(&a),
            r"
                Platforms SHOULD overlap (reverse check failed!)
                Platform corners:
                {:?}
                {:?}
                ",
            b.area_corners(),
            a.area_corners()
        );
    }

    #[test_matrix(
        [platform!(1x1 @ 2, 3), platform!(1x1 @ 5, 5)],
        [platform!(1x1 @ 3, 3), platform!(1x1 @ 5, 4)]
    )]
    #[test_matrix(
        [platform!(3x3 @ 5, 5)],
        [
            platform!(1x1 @ 5, 4), platform!(1x1 @ 8, 5),
            platform!(3x3 @ 8, 5), platform!(3x3 @ 5, 8), platform!(3x3 @ 8, 8)
        ]
    )]
    #[test_matrix(
        [platform!(5x5 @ 5, 5)],
        [
            platform!(1x1 @ 5, 4), platform!(1x1 @ 10, 5), platform!(1x1 @ 10, 9),
            platform!(3x3 @ 10, 5), platform!(3x3 @ 2, 6), platform!(3x3 @ 6, 2),
            platform!(5x5 @ 5, 10), platform!(5x5 @ 0, 7), platform!(5x5 @ 7, 0),
        ]
    )]
    fn platform_overlap_no(a: Platform, b: Platform) {
        assert!(
            !a.overlaps(&b),
            r"
                Platforms should NOT overlap
                Platform corners:
                {:?}
                {:?}
                ",
            a.area_corners(),
            b.area_corners()
        );
        assert!(
            !b.overlaps(&a),
            r"
                Platforms should NOT overlap (reverse check failed!)
                Platform corners:
                {:?}
                {:?}
                ",
            b.area_corners(),
            a.area_corners()
        );
    }
}
