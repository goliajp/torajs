//! The two facts every §10.1.9.2 OrdinarySet walk needs at each link
//! — what a cell's [[Prototype]] is, and where its own entries live
//! — for whichever cell shape the link happens to be.
//!
//! The set side used to know only half of the first one. Its walk
//! seeded at the receiver's EXPLICIT [[Prototype]] slot and advanced
//! the same way, so a receiver that never had one — which is every
//! ordinary object, array and function — had no chain at all:
//! `Object.defineProperty(Object.prototype, "zz", {set(){…}});
//! ({}).zz = 1` wrote a fresh own key and never ran the setter, and
//! `Object.prototype.zz` answered the accessor the whole time. The
//! read side closed the same hole twice already (`member_get_own::
//! implicit_proto_parent` for dynobjs, `member_get_symbol::
//! chain_next` for the symbol lane); this is that fact, once, for
//! the write.
//!
//! An implicit link is not a shortcut for "the root". A builtin
//! prototype answers the parent its own clause gives it —
//! `proto_parent_tag` is the one home for which — and everything
//! else answers its family prototype (`recv_proto_family`), which is
//! itself one link below the root for all but %Object.prototype%.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get_layout::CLOSURE_PROPS_OFF;

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout, header
/// flag bit 6) — an `Object.create(null)` shape has no parent at all.
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

unsafe extern "C" {
    /// torajs-meta — a struct cell's class prototype (`__proto_<C>`),
    /// where a class's runtime-computed accessors reify; 0 when the
    /// class tag was never registered.
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
}

/// A cell's [[Prototype]], explicit link or implicit clause, as the
/// next cell to probe — `None` exactly when the chain ends (an
/// explicit null proto, %Object.prototype% itself, or a family whose
/// prototype singleton was never minted).
///
/// # Safety
/// `cell` is a live heap cell.
pub(crate) unsafe fn chain_parent(cell: u64) -> Option<u64> {
    let ptr = cell as *mut c_void;
    let ctag = unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() };
    if ctag == Tag::DynObj as u16 {
        let flags = unsafe { ptr.cast::<u8>().add(6).cast::<u16>().read() };
        if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
            return None;
        }
        if let Some(explicit) = unsafe { crate::member_get_own::user_proto_cell(ptr) } {
            return Some(explicit);
        }
    } else if ctag == Tag::Closure as u16 {
        // Three states, and the middle one is not "no parent": a
        // function that was never re-parented rides the implicit
        // %Function.prototype% clause below.
        match unsafe { crate::member_get_own::closure_user_proto(ptr) } {
            Some(None) => return None,
            Some(Some(explicit)) => return Some(explicit),
            None => {}
        }
    } else if ctag == Tag::Obj as u16 {
        // A struct carries no [[Prototype]] slot; its chain root is
        // the class prototype the class registered.
        let class_tag = unsafe { ptr.cast::<u8>().add(8).cast::<u32>().read() };
        let root = unsafe { __torajs_proto_cell_raw(class_tag as i64) };
        if crate::nanbox::is_cell(root) {
            return Some(root);
        }
    }
    unsafe { implicit_parent(ptr) }
}

/// The implicit half — what a cell with no explicit link inherits
/// from. A builtin prototype singleton is one link INTO the chain
/// already, so it answers its clause's parent; anything else answers
/// its own family prototype.
///
/// # Safety
/// `ptr` is a live heap cell.
unsafe fn implicit_parent(ptr: *mut c_void) -> Option<u64> {
    let bp_tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    let tag = if bp_tag >= 0 {
        torajs_rc::builtin_proto::proto_parent_tag(bp_tag)
    } else {
        crate::method_value::family::recv_proto_family(crate::nanbox::box_void_ptr(ptr))
    };
    if tag < 0 {
        return None;
    }
    let parent = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
    if parent.is_null() || core::ptr::eq(parent.cast_const(), ptr.cast_const()) {
        return None;
    }
    Some(parent as u64)
}

/// The dynobj table a chain link keeps its own non-index entries in.
///
/// `None` is the shapes whose property face this walk does not model
/// (a recorded boundary — the walk stops rather than guessing);
/// `Some(NULL)` is a modeled shape that simply has no table yet, and
/// the walk keeps climbing past it. Collapsing those two stopped the
/// walk at the first function that had never been assigned to.
///
/// # Safety
/// `cell` is a live heap cell.
pub(crate) unsafe fn entry_dict(cell: u64) -> Option<*const c_void> {
    let ptr = cell as *mut c_void;
    let ctag = unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() };
    if ctag == Tag::DynObj as u16 {
        return Some(ptr.cast_const());
    }
    // §20.2.3 makes %Function.prototype% a built-in FUNCTION object
    // and §23.1.3 makes Array.prototype an Array exotic, so neither
    // keeps its entries in itself — both use the same `+24` props
    // slot every ordinary function and array does.
    if ctag == Tag::Closure as u16 || ctag == Tag::Arr as u16 {
        return Some(unsafe { *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const *const c_void) });
    }
    None
}
