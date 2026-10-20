//! A closure env released while it stays alive: it is offered to the
//! cycle collector only when a cycle can pass through it (a trace, or
//! an expando bag), which is what the collector's own walkability gate
//! answers; a bare env is turned away before the call.

mod common;

use core::ffi::c_void;
use std::sync::{Mutex, MutexGuard};

use torajs_cycle::buffer::__torajs_cycle_unbuffer;
use torajs_rc::{FLAG_BUFFERED, HeapHeader, Tag};
use torajs_value_drop::__torajs_value_drop_heap;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// `{ hdr | fn_ptr@8 | drop_fn@16 | props@24 | boxed_entry@32 | trace_fn@40 }`
const WORDS: usize = 6;
const PROPS: usize = 3;
const TRACE: usize = 5;

unsafe extern "C" fn never_drop(_p: *mut c_void) {
    panic!("the env lost no reference it did not have");
}

/// An env at rc 2 the test owns; `drop_fn` panics, so a release that
/// takes it to zero fails the test.
fn env() -> Box<[usize; WORDS]> {
    let mut cell = Box::new([0usize; WORDS]);
    let h = cell.as_mut_ptr() as *mut HeapHeader;
    unsafe {
        *h = HeapHeader {
            refcount: 2,
            type_tag: Tag::Closure as u16,
            flags: 0,
        };
    }
    cell[2] = never_drop as *const () as usize;
    cell
}

fn flags(cell: &[usize; WORDS]) -> u16 {
    unsafe { (*(cell.as_ptr() as *const HeapHeader)).flags }
}

fn released_alive(cell: &mut [usize; WORDS]) -> bool {
    let p = cell.as_mut_ptr() as *mut c_void;
    unsafe { __torajs_value_drop_heap(p) };
    assert_eq!(cell[0] as u32, 1, "one reference was released");
    let buffered = flags(cell) & FLAG_BUFFERED != 0;
    assert_eq!(buffered, torajs_cycle::buffer::len() == 1);
    unsafe { __torajs_cycle_unbuffer(p) };
    assert_eq!(torajs_cycle::buffer::len(), 0);
    buffered
}

#[test]
fn a_bare_env_is_not_a_candidate_root() {
    let _s = serial();
    let mut cell = env();
    assert!(!released_alive(&mut cell));
}

#[test]
fn an_env_with_a_trace_is_offered() {
    let _s = serial();
    let mut cell = env();
    cell[TRACE] = never_drop as *const () as usize;
    assert!(released_alive(&mut cell));
}

#[test]
fn an_env_with_a_bag_is_offered() {
    let _s = serial();
    let mut cell = env();
    let bag = [0usize; 2];
    cell[PROPS] = bag.as_ptr() as usize;
    assert!(released_alive(&mut cell));
}
