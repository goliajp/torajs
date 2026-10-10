//! The inspect depth limit: a container nested more than two levels
//! below the logged value prints as a placeholder instead of its
//! contents, the way bun (1.4.3) and node's `util.inspect` default
//! `depth: 2` do. A non-empty array is `[Array ...]`, any object
//! (a dynamic object, a class instance or struct cell, empty or not)
//! `[Object ...]`, a non-empty Map / Set `[Map ...]` / `[Set ...]`.
//! An empty array, Map or Set still prints itself (`[]`, `Map {}`);
//! every other kind (functions, dates, regexps, typed arrays,
//! promises, primitives) prints as it would anywhere else.

use core::ffi::c_void;

use torajs_rc::Tag;

/// The indent column of a value three levels below the logged one:
/// every container level adds 2.
pub(super) const CUT_INDENT: u32 = 6;

const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_map_size(p: *const c_void) -> i64;
}

/// The placeholder `child` (a heap cell of `tag`) prints as at an
/// indent past the depth limit, or `None` when it prints itself.
///
/// # Safety
/// `child` is a live heap cell whose header carries `tag`.
pub(super) unsafe fn placeholder(child: *const c_void, tag: u16) -> Option<&'static [u8]> {
    if tag == Tag::Arr as u16 {
        let len = unsafe { child.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        (len > 0).then_some(b"[Array ...]")
    } else if tag == Tag::DynObj as u16 || tag == Tag::Obj as u16 {
        Some(b"[Object ...]")
    } else if tag == Tag::Map as u16 {
        (unsafe { __torajs_map_size(child) } > 0).then_some(b"[Map ...]")
    } else if tag == Tag::Set as u16 {
        (unsafe { __torajs_map_size(child) } > 0).then_some(b"[Set ...]")
    } else {
        None
    }
}
