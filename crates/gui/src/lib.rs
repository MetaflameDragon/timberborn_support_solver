pub mod app;

pub use crate::app::App;

// ----------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::*;
