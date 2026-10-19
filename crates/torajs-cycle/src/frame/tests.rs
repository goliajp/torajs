use super::*;
use crate::children::{clear_child_slot, for_each_child};
use crate::layout::has_walkable_children;
use crate::promise::{for_each_promise_child, promise_slot_clear};

#[repr(C, align(8))]
struct Cell([u64; 8]);

fn cell(tag: u16) -> Box<Cell> {
    Box::new(Cell([(tag as u64) << 32 | 1, 0, 0, 0, 0, 0, 0, 0]))
}

fn at(c: &mut Cell) -> *mut c_void {
    c as *mut Cell as *mut c_void
}

/// The shape a synthesized frame trace has: `result` (+32), the
/// `awaiting` word (+48) unless it carries the parked bit, then the
/// live slots (+56, +64), NULL ones included, each with its address.
unsafe extern "C" fn trace(
    p: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    let w = p as *mut *mut c_void;
    unsafe {
        visit(0, *w.add(4), w.add(4) as *mut c_void, ctx);
        if *w.add(6) as usize & 1 == 0 {
            visit(1, *w.add(6), w.add(6) as *mut c_void, ctx);
        }
        for i in 0..2 {
            visit(
                2 + i as i64,
                *w.add(7 + i),
                w.add(7 + i) as *mut c_void,
                ctx,
            );
        }
    }
}

unsafe extern "C" fn marks_dropped(p: *mut c_void) {
    unsafe { *(p as *mut u64).add(2) = 0xd0d0 };
}

#[repr(C)]
struct Desc([usize; 3]);

fn desc(traced: bool) -> Box<Desc> {
    let t = if traced {
        trace as *const () as usize
    } else {
        0
    };
    Box::new(Desc([0, marks_dropped as *const () as usize, t]))
}

/// A frame cell long enough for two slots.
#[repr(C, align(8))]
struct Frame([u64; 9]);

fn frame(d: &mut Desc) -> Box<Frame> {
    let mut f = Box::new(Frame([0; 9]));
    f.0[0] = (TAG_ASYNC_FRAME as u64) << 32 | 1;
    f.0[1] = d as *mut Desc as u64;
    f
}

fn fp(f: &mut Frame) -> *mut c_void {
    f as *mut Frame as *mut c_void
}

fn children(p: *mut c_void) -> Vec<(u64, *mut c_void)> {
    let mut seen = Vec::new();
    unsafe { for_each_child(p, |i, c| seen.push((i, c))) };
    seen
}

#[test]
fn a_frame_is_walked_through_its_trace() {
    let mut d = desc(true);
    let mut f = frame(&mut d);
    let (mut r, mut a, mut s) = (cell(8), cell(8), cell(1));
    let (rp, ap, sp) = (at(&mut r), at(&mut a), at(&mut s));
    f.0[4] = rp as u64;
    f.0[6] = ap as u64;
    f.0[7] = sp as u64;
    let p = fp(&mut f);
    assert!(unsafe { has_walkable_children(p) });
    assert_eq!(children(p), vec![(0, rp), (1, ap), (2, sp)]);
    // parked: the same word is a back-pointer, which the trace skips
    f.0[6] = ap as u64 | 1;
    assert_eq!(children(p), vec![(0, rp), (2, sp)]);
}

#[test]
fn clearing_each_yielded_slot_breaks_exactly_that_edge() {
    let mut d = desc(true);
    let mut f = frame(&mut d);
    let (mut r, mut a, mut s) = (cell(8), cell(8), cell(1));
    f.0[4] = at(&mut r) as u64;
    f.0[6] = at(&mut a) as u64;
    f.0[7] = at(&mut s) as u64;
    let p = fp(&mut f);
    for (i, _) in children(p) {
        unsafe { clear_child_slot(p, i) };
    }
    assert_eq!((f.0[6], f.0[4], f.0[7]), (0, 0, 0));
    assert_eq!(f.0[1], &mut *d as *mut Desc as u64, "desc is not an edge");
    assert!(children(p).is_empty());
}

/// Every frame's desc has a trace; a debug build says so where the
/// walk first asks.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "has no trace")]
fn a_frame_without_a_trace_is_refused_in_a_debug_build() {
    let mut leaf = desc(false);
    let mut f = frame(&mut leaf);
    unsafe { has_walkable_children(fp(&mut f)) };
}

/// The walkability answer before the closure and frame arms were
/// split off: every predicate in turn.
unsafe fn walkable_by_every_predicate(p: *mut c_void) -> bool {
    use crate::layout::{
        is_class_obj, is_visitable_arr, is_visitable_closure, is_visitable_dynobj,
        is_visitable_wrapper, nan_box_is_cell_like,
    };
    nan_box_is_cell_like(p)
        && unsafe {
            is_class_obj(p)
                || is_visitable_arr(p)
                || is_visitable_dynobj(p)
                || is_visitable_wrapper(p)
                || is_visitable_closure(p)
                || crate::layout_bag::is_visitable_bag(p)
                || crate::proxy::is_visitable_proxy(p)
                || crate::accessor::is_visitable_accessor(p)
        }
}

/// Answering a closure or a frame off its tag first leaves every
/// other shape's answer as it was: each tag, each flag word, with an
/// empty body and with every body word naming a (zeroed) cell.
#[test]
fn walkability_by_tag_matches_the_predicate_chain_for_every_other_shape() {
    use crate::layout::{FLAG_STATIC_LITERAL, TAG_OBJ};
    let mut other = Box::new([0u64; 64]);
    let other_p = other.as_mut_ptr() as u64;
    let mut checked = 0;
    for tag in 0..=40u16 {
        if tag == TAG_ASYNC_FRAME {
            continue;
        }
        for flags in [0u16, FLAG_STATIC_LITERAL] {
            for filled in [false, true] {
                // a class instance with a class tag reads the codegen
                // layout table; both answers call the same predicate
                if filled && tag == TAG_OBJ {
                    continue;
                }
                let mut c = Box::new([0u64; 16]);
                c[0] = (flags as u64) << 48 | (tag as u64) << 32 | 1;
                if filled {
                    c[1..].fill(other_p);
                }
                let p = c.as_mut_ptr() as *mut c_void;
                assert_eq!(
                    unsafe { has_walkable_children(p) },
                    unsafe { walkable_by_every_predicate(p) },
                    "tag {tag} flags {flags:#x} filled {filled}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 40 * 4 - 2);
}

#[test]
fn a_corpse_runs_its_drop_and_a_parked_one_reports_its_size() {
    let mut d = desc(true);
    let mut f = frame(&mut d);
    let p = fp(&mut f);
    unsafe { frame_corpse_drop(p) };
    assert_eq!(f.0[2], 0xd0d0);
    f.0[1] = 72;
    assert_eq!(unsafe { parked_frame_size(fp(&mut f)) }, 72);
}

// source -> frame -> derived (inline)
#[test]
fn a_promise_yields_its_parked_frame_and_unlinks_it() {
    let mut d = desc(false);
    let mut f = frame(&mut d);
    let (mut s, mut der) = (cell(8), cell(8));
    let (sp, dp, framep) = (at(&mut s), at(&mut der), fp(&mut f));
    s.0[3] = framep as u64 | 2;
    f.0[5] = dp as u64 | 1;
    f.0[6] = sp as u64 | 1;
    der.0[6] = sp as u64;
    let mut seen = Vec::new();
    unsafe { for_each_promise_child(sp, &mut |_, c| seen.push(c)) };
    assert_eq!(seen, vec![framep, dp]);

    let mut seen = Vec::new();
    unsafe {
        for_each_promise_child(sp, &mut |i, c| {
            seen.push(c);
            promise_slot_clear(sp, i);
        })
    };
    assert_eq!(seen, vec![framep, dp]);
    assert_eq!(s.0[3], 0, "the list is empty");
    assert_eq!((f.0[5], f.0[6]), (0, 0), "the frame names nothing");
    assert_eq!(der.0[6], 0);
}
