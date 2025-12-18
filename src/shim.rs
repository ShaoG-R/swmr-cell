#[cfg(not(feature = "loom"))]
pub use core::cell::Cell;
#[cfg(feature = "loom")]
pub use loom::cell::Cell;

// Atomics (Loom vs Core)
#[cfg(not(feature = "loom"))]
pub use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
#[cfg(feature = "loom")]
pub use loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

#[cfg(not(feature = "loom"))]
mod memory {
    #[cfg(feature = "std")]
    mod inner {
        pub use std::boxed::Box;
        pub use std::collections::VecDeque;
        pub use std::sync::Arc;
        pub use std::vec::Vec;
    }

    #[cfg(not(feature = "std"))]
    mod inner {
        pub use alloc::boxed::Box;
        pub use alloc::collections::VecDeque;
        pub use alloc::sync::Arc;
        pub use alloc::vec::Vec;
    }

    pub use inner::*;
}

#[cfg(feature = "loom")]
mod memory {
    pub use loom::sync::Arc;
    pub use std::boxed::Box;
    pub use std::collections::VecDeque;
    pub use std::vec::Vec;
}

pub use memory::*;

// Lock (Mutex) Abstraction
#[cfg(feature = "loom")]
mod loom_mutex {
    pub struct Mutex<T>(loom::sync::Mutex<T>);

    impl<T> Mutex<T> {
        #[inline]
        pub fn new(t: T) -> Self {
            Self(loom::sync::Mutex::new(t))
        }

        #[inline]
        pub fn lock(&self) -> loom::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap()
        }
    }
}

#[cfg(feature = "loom")]
pub use loom_mutex::Mutex;

#[cfg(not(feature = "loom"))]
mod locks {
    #[cfg(feature = "std")]
    mod std_mutex {
        use std::ops::{Deref, DerefMut};
        use std::sync;

        /// Like `std::sync::Mutex` except that it does not poison itself.
        pub struct Mutex<T: ?Sized>(sync::Mutex<T>);

        impl<T> Mutex<T> {
            #[inline]
            pub fn new(t: T) -> Self {
                Self(sync::Mutex::new(t))
            }
        }

        impl<T: ?Sized> Mutex<T> {
            #[inline]
            pub fn lock(&self) -> MutexGuard<'_, T> {
                MutexGuard(self.0.lock().unwrap_or_else(|e| e.into_inner()))
            }
        }

        pub struct MutexGuard<'a, T: ?Sized + 'a>(sync::MutexGuard<'a, T>);

        impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
            type Target = T;
            #[inline]
            fn deref(&self) -> &T {
                self.0.deref()
            }
        }

        impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
            #[inline]
            fn deref_mut(&mut self) -> &mut T {
                self.0.deref_mut()
            }
        }
    }

    #[cfg(feature = "std")]
    pub use std_mutex::Mutex;

    #[cfg(not(feature = "std"))]
    pub use spin::Mutex;
}

#[cfg(not(feature = "loom"))]
pub use locks::*;

// Ensure a compile error in no_std environment without spin feature.
// 确保在 no_std 且没有 spin 的情况下报错
#[cfg(all(not(feature = "std"), not(feature = "spin"), not(feature = "loom")))]
compile_error!("To use swmr-cell in no_std, you must enable the 'spin' feature or 'loom' feature.");
