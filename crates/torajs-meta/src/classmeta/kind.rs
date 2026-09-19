//! [[ConstructorKind]] by class tag (§10.2.2 step 13.c needs it at a
//! `super(…)` site whose parent is a runtime VALUE): a class with a
//! heritage clause is `derived`, everything else `base`. Recorded by
//! `__torajs_anyv_class_register` from the desugar's own view of the
//! heritage (`class_parents`), which is the one view the rest of the
//! desugar takes — a value-shaped or builtin parent counts, since the
//! kind is fixed at definition and is not the [[Prototype]] link.
//!
//! Read by [`__torajs_class_kind_derived`], a registry READER for the
//! link-time class register judgment (`cmd_build_elide_class::
//! REGISTRY_READERS`), never inlined so the judgment sees every read.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};

use super::{MAX_CLASSES, in_range};

static DERIVED: [AtomicBool; MAX_CLASSES] = [const { AtomicBool::new(false) }; MAX_CLASSES];

pub(super) fn record(tag: i64, derived: i64) {
    if in_range(tag) {
        DERIVED[tag as usize].store(derived != 0, Ordering::Relaxed);
    }
}

/// Is the class object `cell` a derived constructor? `false` for a
/// cell that is no registered class object (a plain function parent
/// is a base constructor per §10.2.2 anyway). One evaluation of a
/// unit class answers for its template.
///
/// # Safety
/// `cell` is a live heap cell or NULL.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_class_kind_derived(cell: *const c_void) -> bool {
    match unsafe { crate::class_object_print::tag_of_class_object(cell) } {
        Some(tag) if in_range(tag) => DERIVED[tag as usize].load(Ordering::Relaxed),
        _ => false,
    }
}
