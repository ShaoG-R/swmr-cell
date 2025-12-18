#[cfg(not(feature = "loom"))]
pub use core::cell::Cell;
#[cfg(feature = "loom")]
pub use loom::cell::Cell;

#[cfg(not(feature = "loom"))]
pub use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
#[cfg(feature = "loom")]
pub use loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

#[cfg(feature = "loom")]
pub use loom::sync::Arc;

#[cfg(not(feature = "loom"))]
#[cfg(feature = "std")]
pub use std::sync::Arc;

#[cfg(not(feature = "loom"))]
#[cfg(not(feature = "std"))]
pub use alloc::sync::Arc;

#[cfg(not(feature = "loom"))]
#[cfg(feature = "std")]
pub use antidote::Mutex;

#[cfg(not(feature = "loom"))]
#[cfg(not(feature = "std"))]
#[cfg(feature = "spin")]
pub use spin::Mutex;

#[cfg(not(feature = "loom"))]
#[cfg(not(feature = "std"))]
#[cfg(not(feature = "spin"))]
compile_error!("To use swmr-cell in no_std, you must enable the 'spin' feature or 'loom' feature.");

#[cfg(feature = "loom")]
#[derive(Debug, Default)]
pub struct Mutex<T>(loom::sync::Mutex<T>);

#[cfg(feature = "loom")]
impl<T> Mutex<T> {
    pub fn new(t: T) -> Self {
        Self(loom::sync::Mutex::new(t))
    }

    pub fn lock(&self) -> loom::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
