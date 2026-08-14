//! This module contains an implementation of a tiny `Arc` without weak
//! references, inspired by the `Arc` implementation in [rclite] (of course,
//! all bugs are mine).
//!
//! [rclite]: https://github.com/fereidani/rclite

use alloc::boxed::Box;
use core::mem::MaybeUninit;
use core::ptr::{NonNull, addr_of_mut};
use core::sync::atomic;

/// A tiny `Arc` without weak references.
pub(super) struct Arc<T> {
    ptr: NonNull<ArcInner<T>>,
}

unsafe impl<T: Sync + Send> Send for Arc<T> {}
unsafe impl<T: Sync + Send> Sync for Arc<T> {}

struct ArcInner<T> {
    counter: atomic::AtomicUsize,
    data: T,
}

unsafe impl<T: Sync + Send> Send for ArcInner<T> {}
unsafe impl<T: Sync + Send> Sync for ArcInner<T> {}

impl<T> Arc<T> {
    #[inline]
    pub(super) fn get_mut(this: &mut Self) -> Option<&mut T> {
        if this.is_unique() {
            // SAFETY: this is the only `Arc` pointing to the inner value, so
            // we can safely mutate it.
            unsafe { Some(Self::get_mut_unchecked(this)) }
        } else {
            None
        }
    }

    #[inline]
    pub(super) unsafe fn get_mut_unchecked(this: &mut Self) -> &mut T {
        unsafe { &mut this.ptr.as_mut().data }
    }

    #[inline]
    fn inner(&self) -> &ArcInner<T> {
        // SAFETY: the inner pointer is valid as long as there's at least one
        // `Arc` pointing to it.
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.inner().counter.load(atomic::Ordering::Acquire) == 1
    }

    #[inline]
    pub(super) fn new(data: T) -> Self {
        let inner = ArcInner { counter: atomic::AtomicUsize::new(1), data };

        // SAFETY: the pointer returned by `Box::into_raw()` is guaranteed to
        // be non-null.
        let ptr =
            unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(inner))) };

        Self { ptr }
    }

    #[inline]
    pub(super) fn ptr_eq(this: &Self, other: &Self) -> bool {
        this.ptr == other.ptr
    }
}

impl<T: Clone> Arc<T> {
    #[inline]
    pub(super) fn make_mut(this: &mut Self) -> &mut T {
        if !this.is_unique() {
            *this = this.optimized_clone();
        }

        // SAFETY: either the reference was unique or we just cloned the T.
        unsafe { Self::get_mut_unchecked(this) }
    }

    #[inline]
    fn optimized_clone(&self) -> Self {
        // See the homonymous function in `rclite` for more details.

        let mut buffer: Box<MaybeUninit<ArcInner<T>>> =
            Box::new(MaybeUninit::uninit());

        let ptr = unsafe {
            let ptr = buffer.as_mut_ptr();
            // Here we use `write()` instead of assignment via `=` to avoid
            // dropping the old, uninitialized value.
            addr_of_mut!((*ptr).data).write(T::clone(self));
            (*ptr).counter = atomic::AtomicUsize::new(1);
            NonNull::new_unchecked(Box::into_raw(buffer) as *mut ArcInner<T>)
        };

        Arc { ptr }
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Arc<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, f)
    }
}

impl<T: Default> Default for Arc<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Clone for Arc<T> {
    #[inline]
    fn clone(&self) -> Self {
        let old = self.inner().counter.fetch_add(1, atomic::Ordering::Relaxed);

        // Check for overflow on the counter. See the `Arc` implementation in
        // `alloc` for more details.
        if unlikely(old > isize::MAX as usize) {
            drop(Self { ptr: self.ptr });
            panic!("Arc counter overflow");
        }

        Self { ptr: self.ptr }
    }
}

impl<T> core::ops::Deref for Arc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().data
    }
}

impl<T> Drop for Arc<T> {
    #[inline]
    fn drop(&mut self) {
        let old = self.inner().counter.fetch_sub(1, atomic::Ordering::Release);

        if old == 1 {
            atomic::fence(atomic::Ordering::Acquire);

            // SAFETY: this is the last owner of the `Arc` so the memory has
            // not yet been reclaimed by a previous call to `Box::from_raw()`.
            let _ = unsafe { Box::from_raw(self.ptr.as_ptr()) };
        }
    }
}

use predictions::*;

mod predictions {
    //! Hints for branch prediction.

    #[inline(always)]
    pub(super) fn unlikely(b: bool) -> bool {
        if b {
            cold();
        }
        b
    }

    #[inline(always)]
    #[cold]
    fn cold() {}
}
