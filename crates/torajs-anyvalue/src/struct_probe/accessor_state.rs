//! What a DECLARED object-literal accessor member is now. A delete
//! records a HOLE in the member's `+24` sidecar (§10.1.10), and a
//! store after it creates an ordinary data property there
//! (§10.1.9.2 → CreateDataProperty) that is an expando from then on.
//! torajs-meta `__torajs_obj_accessor_state` reads the sidecar's
//! value word and is the one judgment; this is the any lane's spelling
//! of its answer.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-meta — live / deleted / replaced; DELETED and REPLACED
    /// only for a name the layout declares as an accessor.
    fn __torajs_obj_accessor_state(cell: *const c_void, name: *const u8, name_len: u32) -> i64;
}

/// torajs-meta `ACCESSOR_*` mirrors.
const ACCESSOR_DELETED: i64 = 1;
const ACCESSOR_REPLACED: i64 = 2;

/// The state of the accessor member `prop` of a struct cell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessorState {
    /// An own accessor — or no accessor record under the name at all.
    Live,
    /// Deleted: no own property under the name.
    Deleted,
    /// An ordinary property a store or define created after the
    /// delete; its dict entry is that property.
    Replaced,
}

/// What the accessor member `prop` of `ptr` is now. An instance nothing
/// redefined or deleted answers [`AccessorState::Live`] for one header
/// read.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell.
pub(crate) unsafe fn accessor_state(ptr: *const c_void, prop: &[u8]) -> AccessorState {
    if !unsafe { crate::member_get_layout::header_flag(ptr, torajs_rc::FLAG_OBJ_EXOTIC_FIELD) } {
        return AccessorState::Live;
    }
    match unsafe { __torajs_obj_accessor_state(ptr, prop.as_ptr(), prop.len() as u32) } {
        ACCESSOR_DELETED => AccessorState::Deleted,
        ACCESSOR_REPLACED => AccessorState::Replaced,
        _ => AccessorState::Live,
    }
}
