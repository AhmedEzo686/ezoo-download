//! Locks that keep working after a thread panicked while holding them.
//!
//! A plain `lock().unwrap()` turns one panic into a panic in every later
//! user of the same lock. The data behind our locks is always left in a
//! usable state (lists, counters, settings), so taking it over is safe.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// `lock()` that never panics.
pub trait LockOk<T: ?Sized> {
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockOk<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// `read()` and `write()` that never panic.
pub trait RwOk<T: ?Sized> {
    fn read_ok(&self) -> RwLockReadGuard<'_, T>;
    fn write_ok(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwOk<T> for RwLock<T> {
    fn read_ok(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn write_ok(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_lock_still_works_after_a_panic() {
        let m = Arc::new(Mutex::new(5));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("on purpose");
        })
        .join();
        assert!(m.is_poisoned());
        *m.lock_ok() += 1;
        assert_eq!(*m.lock_ok(), 6);
        let r = RwLock::new(1);
        *r.write_ok() = 2;
        assert_eq!(*r.read_ok(), 2);
    }
}
