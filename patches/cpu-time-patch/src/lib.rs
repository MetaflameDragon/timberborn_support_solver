//! A temporary no-op patch crate for cpu-time to work with WASM

#![warn(missing_debug_implementations)]
#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{
        io::Result,
        marker::PhantomData,
        rc::Rc,
        time::{Duration, Instant},
    };

    #[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
    pub struct ProcessTime(Instant);

    #[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
    pub struct ThreadTime(
        Instant,
        // makes type non-sync and non-send
        PhantomData<Rc<()>>,
    );

    impl ProcessTime {
        pub fn try_now() -> Result<Self> {
            Ok(Self(Instant::now()))
        }

        pub fn now() -> Self {
            Self::try_now().unwrap()
        }

        pub fn try_elapsed(&self) -> Result<Duration> {
            Ok(Self::try_now()?.duration_since(*self))
        }

        pub fn elapsed(&self) -> Duration {
            Self::now().duration_since(*self)
        }

        pub fn duration_since(&self, timestamp: Self) -> Duration {
            self.0.duration_since(timestamp.0)
        }

        pub fn as_instant(&self) -> Instant {
            self.0
        }
    }

    impl ThreadTime {
        pub fn try_now() -> Result<Self> {
            Ok(Self(Instant::now(), PhantomData::default()))
        }

        pub fn now() -> Self {
            Self::try_now().unwrap()
        }

        pub fn try_elapsed(&self) -> Result<Duration> {
            Ok(Self::try_now()?.duration_since(*self))
        }

        pub fn elapsed(&self) -> Duration {
            Self::now().duration_since(*self)
        }

        pub fn duration_since(&self, timestamp: ThreadTime) -> Duration {
            self.0.duration_since(timestamp.0)
        }

        pub fn as_instant(&self) -> Instant {
            self.0
        }
    }
}
