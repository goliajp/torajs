//! Explicit work stack for the collector's graph walks.
//!
//! The three phases in [`collect`](crate::collect) visit every cell
//! reachable from the buffered roots. Walking that graph on the
//! machine stack overflows on a long enough chain (a linked list a
//! few hundred thousand nodes deep is an ordinary program), so each
//! phase keeps its pending cells here instead: a growable array of
//! words on the runtime allocator, with no depth limit.
//!
//! One stack serves a whole collect. Collects are not reentrant
//! (`IN_COLLECT`), so the backing array is handed out by [`take`] and
//! returned when the [`WorkStack`] drops; a small one is kept for the
//! next collect, a large one goes back to the allocator.
//!
//! [`take`]: WorkStack::take

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

unsafe extern "C" {
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free_sized(p: *mut c_void, size: usize);
}

/// Entries a returned stack may keep for the next collect (32 KiB).
const RETAIN_CAP: usize = 4096;

const FIRST_CAP: usize = 64;

static W_BUF: AtomicPtr<usize> = AtomicPtr::new(ptr::null_mut());
static W_CAP: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct WorkStack {
    buf: *mut usize,
    len: usize,
    cap: usize,
}

impl WorkStack {
    /// Borrow the retained backing array, empty.
    pub(crate) fn take() -> Self {
        let buf = W_BUF.swap(ptr::null_mut(), Ordering::Relaxed);
        let cap = W_CAP.swap(0, Ordering::Relaxed);
        Self { buf, len: 0, cap }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    // out of line: the walks push from inside `for_each_child`, whose
    // every shape arm would otherwise carry its own copy
    #[inline(never)]
    pub(crate) fn push(&mut self, v: usize) {
        if self.len == self.cap {
            self.grow();
        }
        unsafe { *self.buf.add(self.len) = v };
        self.len += 1;
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(unsafe { *self.buf.add(self.len) })
    }

    /// Pop an entry pushed after the stack was `base` long.
    #[inline]
    pub(crate) fn pop_above(&mut self, base: usize) -> Option<usize> {
        if self.len == base {
            return None;
        }
        self.pop()
    }

    /// Reverse the entries pushed since the stack was `base` long, so
    /// they pop in the order they were pushed.
    #[inline]
    pub(crate) fn reverse_from(&mut self, base: usize) {
        if self.len - base > 1 {
            unsafe { core::slice::from_raw_parts_mut(self.buf.add(base), self.len - base) }
                .reverse();
        }
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self) {
        let new_cap = if self.cap == 0 {
            FIRST_CAP
        } else {
            self.cap * 2
        };
        let word = core::mem::size_of::<usize>();
        self.buf = unsafe { realloc(self.buf as *mut c_void, self.cap * word, new_cap * word) }
            as *mut usize;
        self.cap = new_cap;
    }
}

impl Drop for WorkStack {
    fn drop(&mut self) {
        if self.cap > RETAIN_CAP {
            unsafe {
                free_sized(
                    self.buf as *mut c_void,
                    self.cap * core::mem::size_of::<usize>(),
                )
            };
            return;
        }
        W_BUF.store(self.buf, Ordering::Relaxed);
        W_CAP.store(self.cap, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_past_retain_cap_and_pops_lifo() {
        let mut st = WorkStack::take();
        let n = RETAIN_CAP * 3 + 5;
        for i in 0..n {
            st.push(i << 1);
        }
        assert_eq!(st.len(), n);
        for i in (0..n).rev() {
            assert_eq!(st.pop(), Some(i << 1));
        }
        assert_eq!(st.pop(), None);
    }

    #[test]
    fn reverse_from_keeps_the_prefix() {
        let mut st = WorkStack::take();
        st.push(1);
        st.push(2);
        let base = st.len();
        st.push(3);
        st.push(4);
        st.push(5);
        st.reverse_from(base);
        let mut out = Vec::new();
        while let Some(v) = st.pop() {
            out.push(v);
        }
        assert_eq!(out, vec![3, 4, 5, 2, 1]);
    }
}
