use crate::grid::Grid;

#[derive(Clone, Debug)]
pub struct World {
    terrain_grid: Grid<bool>,
}

impl World {
    pub fn new(terrain_grid: Grid<bool>) -> Self {
        World { terrain_grid }
    }

    pub fn terrain_grid(&self) -> &Grid<bool> {
        &self.terrain_grid
    }
}
