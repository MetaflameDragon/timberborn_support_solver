//! A temporary no-op patch crate for cpu-time to work with WASM

#![allow(unused_variables, dead_code)]

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::{io::Result, marker::PhantomData, rc::Rc, time::Duration};

    #[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
    pub struct ProcessTime;

    #[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
    pub struct ThreadTime(
        // makes type non-sync and non-send
        PhantomData<Rc<()>>,
    );

    impl ProcessTime {
        pub fn try_now() -> Result<Self> {
            Ok(Self)
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
            Duration::ZERO
        }
    }

    impl ThreadTime {
        pub fn try_now() -> Result<Self> {
            Ok(Self(PhantomData::default()))
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
            Duration::ZERO
        }
    }
}
