//! Utilities for working with platforms.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{dimensions::Dimensions, point::Point};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(Serialize, Deserialize)]
pub struct PlatformDef {
    dims: Dimensions,
}

#[macro_export]
macro_rules! platform_def {
    ($x:literal, $y:literal) => {
        PlatformDef::new(Dimensions::new($x, $y))
    };
}

pub const PLATFORMS_DEFAULT: [PlatformDef; 8] = [
    platform_def!(1, 1),
    platform_def!(1, 2),
    platform_def!(1, 3),
    platform_def!(1, 4),
    platform_def!(1, 5),
    platform_def!(1, 6),
    platform_def!(3, 3),
    platform_def!(5, 5),
];

impl PlatformDef {
    pub const fn new(dims: Dimensions) -> Self {
        PlatformDef { dims }
    }

    /// The outer corner of the area this platform covers (relative to the
    /// origin).
    ///
    /// The first corner is at (0, 0), and the corner point is inclusive.
    pub const fn dims(self) -> Dimensions {
        self.dims
    }

    pub fn dimensions_str(self) -> String {
        format!("{}x{}", self.dims.width(), self.dims.height())
    }

    pub const fn rectangular(self) -> bool {
        self.dims().width != self.dims().height
    }
}

impl Display for PlatformDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dimensions_str())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(Serialize, Deserialize)]
pub struct Platform {
    point: Point,
    def: PlatformDef,
    rotated: bool,
}

impl Platform {
    pub fn new(point: Point, def: PlatformDef, rotated: bool) -> Self {
        Self { point, def, rotated }
    }

    /// Two corners of the area this platform covers (relative to this
    /// platform's position).
    ///
    /// Both points are inclusive, and `0.x <= 1.x && 0.y <= 1.y`.
    ///
    /// This is better than referring to a platform's inner `point` directly,
    /// since the point may be placed arbitrarily.
    pub fn area_corners(&self) -> Option<(Point, Point)> {
        Some((self.point, self.dims().corner_point_incl()? + self.point))
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        let (Some((self_near, self_far)), Some((other_near, other_far))) =
            (self.area_corners(), other.area_corners())
        else {
            return false;
        };

        other_far.x >= self_near.x
            && other_far.y >= self_near.y
            && other_near.x <= self_far.x
            && other_near.y <= self_far.y
    }

    /// The top-left (min-xy) point of this platform.
    pub fn point(&self) -> Point {
        self.point
    }

    pub fn rotated(&self) -> bool {
        self.rotated
    }

    /// Platform dimensions, taking rotation into account.
    ///
    /// Use `.def().dims()` to get the raw definition dimensions.
    pub fn dims(&self) -> Dimensions {
        if self.rotated() { self.def.dims().flipped() } else { self.def.dims() }
    }

    pub fn def(&self) -> PlatformDef {
        self.def
    }
}

#[allow(unused_macros)]
macro_rules! platform {
    (1x1 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), platform_def!(1, 1))
    };
    (1x2 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), platform_def!(1, 2))
    };
    (2x1 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), platform_def!(2, 1))
    };
    (3x3 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), platform_def!(3, 3))
    };
    (5x5 @ $x:literal, $y:literal) => {
        Platform::new(Point::new($x, $y), platform_def!(5, 5))
    };
}

#[cfg(test)]
mod tests {
    //noinspection RsUnusedImport
    use test_case::{test_case, test_matrix};

    use super::*;

    // TODO: test other platform types?

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
