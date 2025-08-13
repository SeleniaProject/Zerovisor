pub mod format;

pub mod spinlock {
    #![allow(dead_code)]
    use core::cell::UnsafeCell;
    use core::sync::atomic::{AtomicBool, Ordering};

    pub struct SpinLock<T> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }

    unsafe impl<T: Send> Send for SpinLock<T> {}
    unsafe impl<T: Send> Sync for SpinLock<T> {}

    impl<T> SpinLock<T> {
        pub const fn new(v: T) -> Self { Self { locked: AtomicBool::new(false), value: UnsafeCell::new(v) } }
        pub fn lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
            while self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
                core::hint::spin_loop();
            }
            let r = unsafe { f(&mut *self.value.get()) };
            self.locked.store(false, Ordering::Release);
            r
        }
    }
}



