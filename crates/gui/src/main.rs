// TODO: remake a proper backend for native

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::run::<rustsat_glucose::simp::Glucose>()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    panic!("This shouldn't be compiled for WASM!");
}
