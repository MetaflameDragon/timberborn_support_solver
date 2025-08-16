use std::{fmt::Formatter, iter};

use assertables::assert_le;
use derive_more::{Deref, DerefMut};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error, SeqAccess, Unexpected, Visitor},
    ser::SerializeSeq,
};

use crate::{grid::Grid, math::Dimensions};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    grid: WorldGrid,
}

#[derive(Clone, Debug, Deref, DerefMut)]
pub struct WorldGrid(pub Grid<bool>);

impl Serialize for WorldGrid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.dims().height))?;
        for row in self.iter_rows() {
            let row_str = row
                .iter()
                .map(|b| match b {
                    true => "X",
                    false => " ",
                })
                .collect::<String>();
            seq.serialize_element(&row_str)?;
        }
        seq.end()
    }
}

struct WorldGridVisitor;
impl<'de> Visitor<'de> for WorldGridVisitor {
    type Value = WorldGrid;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str(r#"an array of "X" and " " characters forming a grid"#)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let rows = iter::from_fn(move || seq.next_element::<String>().transpose())
            .map(|row_str| -> Result<Vec<bool>, A::Error> {
                row_str?
                    .chars()
                    .map(|c| match c {
                        ' ' => Ok(false),
                        'X' => Ok(true),
                        c => Err(Error::invalid_value(Unexpected::Char(c), &r#"`X` or ` `"#)),
                    })
                    .collect::<Result<_, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Err(Error::invalid_length(0, &"1 or more"));
        }
        let max_row_len = rows.iter().map(Vec::len).max().unwrap();
        let dims = Dimensions::new(max_row_len, rows.len());

        let mut grid_vec = vec![false; dims.width * dims.height];
        for (grid_row, de_row) in grid_vec.chunks_exact_mut(dims.width).zip(rows) {
            assert_le!(de_row.len(), dims.width);
            grid_row.copy_from_slice(&de_row);
        }

        let grid = Grid::try_from_vec(dims, grid_vec).expect("failed to create grid unexpectedly");
        Ok(WorldGrid(grid))
    }
}

/// WorldGrid can be deserialized from an array of strings,
/// where `X` and ` ` map to true and false.
///
/// The lengths need not be identical, the strings are treated as left-aligned
/// and undefined tiles default to false.
impl<'de> Deserialize<'de> for WorldGrid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(WorldGridVisitor)
    }
}

impl World {
    pub fn new(grid: WorldGrid) -> Self {
        World { grid }
    }

    pub fn grid(&self) -> &WorldGrid {
        &self.grid
    }
}
