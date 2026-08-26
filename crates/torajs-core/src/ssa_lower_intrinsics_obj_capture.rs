//! Pass 0 `declare_intrinsic` group: obj alloc/drop + cycle unbuffer +
//! capture-box rc.
//!
//! chunk 111 scale-up of the multi-sub-sibling Pass 0 split that began
//! in `ssa_lower_intrinsics_print_str` (chunk 110). 7 declarations:
//!
//! - `__torajs_obj_alloc(size)` / `__torajs_obj_drop_sized(p, size)` —
//!   P2.4.c heap alloc + Phase 2e item 14 sized drop ABI. Callsite
//!   knows the alloc byte size (env block `CLOSURE_CAP_BASE_OFF +
//!   N_caps*8`, typed Obj `OBJ_HEADER_SIZE + N_fields*8`) so passing
//!   it inline lets the runtime bucket the TLAB.push without a
//!   SpanRegistry lookup or SHIM_HEADER read.
//! - `__torajs_value_drop_heap(p)` — V3-05 runtime tag-dispatched
//!   drop, routed by `emit_drop_value`'s recursion guard when a
//!   self-referential class field would inline the same Obj drop a
//!   second time. V3-09 wires class_layouts metadata for proper child
//!   drops.
//! - `__torajs_cycle_unbuffer(p)` — V3-10.b scrub of the cycle buffer
//!   before `p`'s memory is freed. Inline drop emits the call only
//!   when sid is a declared class (anonymous structs never enter the
//!   buffer).
//! - `__torajs_capture_box_alloc(size) -> ptr` / `_inc(p)` / `_drop(p)`
//!   — T-15.g.5 refcounted capture box for escape-captured Copy lets
//!   (number / boolean). Replaces the previous `obj_alloc(8) + Store
//!   init_val` pair so the box can be safely shared across multiple
//!   capturing closures. Layout: 8-byte rc header + 8-byte value; the
//!   returned pointer points at the VALUE slot so all existing
//!   Load/Store sites in the body still use offset 0.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ObjCaptureIds {
    pub obj_alloc: FuncId,
    pub obj_drop_sized: FuncId,
    pub value_drop_heap: FuncId,
    pub cycle_unbuffer: FuncId,
    pub closure_drop_props_slow: FuncId,
    pub closure_unbuffer_slow: FuncId,
    pub capture_box_alloc: FuncId,
    pub capture_box_inc: FuncId,
    pub capture_box_drop: FuncId,
    pub capture_box_drop_heap: FuncId,
    pub capture_box_drop_any: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> ObjCaptureIds {
    ObjCaptureIds {
        obj_alloc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_alloc",
            &[Type::I64],
            Type::Ptr,
        ),
        obj_drop_sized: declare_intrinsic(
            module,
            fn_table,
            "__torajs_obj_drop_sized",
            &[Type::Ptr, Type::I64],
            Type::Void,
        ),
        value_drop_heap: declare_intrinsic(
            module,
            fn_table,
            "__torajs_value_drop_heap",
            &[Type::Ptr],
            Type::Void,
        ),
        cycle_unbuffer: declare_intrinsic(
            module,
            fn_table,
            "__torajs_cycle_unbuffer",
            &[Type::Ptr],
            Type::Void,
        ),
        // A5 (RFC 20260824-s2-5 刀 4) — the closure env-drop's two
        // speculative legs behind link seams (defaults in
        // torajs-dispatch): the props-bag release and the cycle
        // buffer scrub of a FLAG_BUFFERED cell.
        closure_drop_props_slow: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_drop_props_slow",
            &[Type::Ptr],
            Type::Void,
        ),
        closure_unbuffer_slow: declare_intrinsic(
            module,
            fn_table,
            "__torajs_closure_unbuffer_slow",
            &[Type::Ptr],
            Type::Void,
        ),
        capture_box_alloc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_capture_box_alloc",
            &[Type::I64],
            Type::Ptr,
        ),
        capture_box_inc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_capture_box_inc",
            &[Type::Ptr],
            Type::Void,
        ),
        capture_box_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_capture_box_drop",
            &[Type::Ptr],
            Type::Void,
        ),
        capture_box_drop_heap: declare_intrinsic(
            module,
            fn_table,
            "__torajs_capture_box_drop_heap",
            &[Type::Ptr],
            Type::Void,
        ),
        capture_box_drop_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_capture_box_drop_any",
            &[Type::Ptr],
            Type::Void,
        ),
    }
}
