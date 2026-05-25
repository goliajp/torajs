//! Pass C.5 + C.6 — emit per-class LLVM globals:
//!   - `__vtable_<C>` array of method-impl fn pointers (T-24)
//!   - `__torajs_class_layouts` array of `(n_children, ptr offsets[])`
//!     entries + `__torajs_n_class_layouts` length (T-26.C; cycle
//!     collector reads these per class_tag - 1).
//!
//! Extracted from `compile_for_kind_impl` in `ssa_inkwell.rs`
//! (2026-05-25, god-file decomp batch 22b).
//!
//! Both helpers are `pub(super)` so the orchestration in
//! `ssa_inkwell.rs` can call them after Pass C builds `fn_map`.

use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::{FunctionValue, GlobalValue};

use crate::ssa::Module;

/// Pass C.5 (T-24) — emit `__vtable_<C>` globals (`[N x ptr]`
/// constants populated with method-impl fn pointers per class).
/// Slots with no impl in C's MRO get null. Returns the
/// `class_name -> GlobalValue` map for downstream
/// `GlobalRef("__vtable_<C>")` lookup.
pub(super) fn emit_vtable_globals<'ctx>(
    ctx: &'ctx Context,
    llvm_module: &LlvmModule<'ctx>,
    ssa_module: &Module,
    fn_map: &[FunctionValue<'ctx>],
) -> HashMap<String, GlobalValue<'ctx>> {
    let mut out = HashMap::new();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    for vt in &ssa_module.vtable_globals {
        let n = vt.fn_ids.len();
        let arr_t = ptr_t.array_type(n as u32);
        let elems: Vec<inkwell::values::PointerValue> = vt
            .fn_ids
            .iter()
            .map(|opt| match opt {
                Some(fid) => fn_map[fid.0 as usize].as_global_value().as_pointer_value(),
                None => ptr_t.const_null(),
            })
            .collect();
        let arr = ptr_t.const_array(&elems);
        let g = llvm_module.add_global(arr_t, None, &format!("__vtable_{}", vt.class_name));
        g.set_initializer(&arr);
        g.set_constant(true);
        g.set_linkage(inkwell::module::Linkage::Private);
        g.set_unnamed_addr(true);
        out.insert(format!("__vtable_{}", vt.class_name), g);
    }
    out
}

/// Pass C.6 (T-26.C) — emit per-class children-offset tables for
/// the cycle collector. Two globals:
///   `__torajs_class_layouts`        — `[N x { u32 n; ptr offsets }]`
///   `__torajs_n_class_layouts`      — `u32 = N`
/// The runtime indexes by `class_tag - 1`; collector reads each
/// entry's `offsets[]` to enumerate refcounted-pointer fields
/// during mark/scan/collect.
///
/// Each entry's offsets array is itself a private constant `[K x i32]`
/// global; the entry holds a pointer to it. `K` can be 0 (class
/// has no refcounted fields → entry is `{0, NULL}`).
pub(super) fn emit_class_layouts<'ctx>(
    ctx: &'ctx Context,
    llvm_module: &LlvmModule<'ctx>,
    ssa_module: &Module,
) {
    let i32_t = ctx.i32_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let entry_t = ctx.struct_type(&[i32_t.into(), ptr_t.into()], false);
    let n = ssa_module.class_layouts.len();
    let mut entries: Vec<inkwell::values::StructValue> = Vec::with_capacity(n);
    for (i, layout) in ssa_module.class_layouts.iter().enumerate() {
        let offsets_ptr = if layout.child_offsets.is_empty() {
            ptr_t.const_null()
        } else {
            let arr_t = i32_t.array_type(layout.child_offsets.len() as u32);
            let consts: Vec<inkwell::values::IntValue> = layout
                .child_offsets
                .iter()
                .map(|o| i32_t.const_int(*o as u64, false))
                .collect();
            let arr = i32_t.const_array(&consts);
            let g = llvm_module.add_global(arr_t, None, &format!(".__class_offsets_{i}"));
            g.set_initializer(&arr);
            g.set_constant(true);
            g.set_linkage(inkwell::module::Linkage::Private);
            g.set_unnamed_addr(true);
            g.as_pointer_value()
        };
        let n_children = i32_t.const_int(layout.child_offsets.len() as u64, false);
        let entry = ctx.const_struct(&[n_children.into(), offsets_ptr.into()], false);
        entries.push(entry);
    }
    let table_t = entry_t.array_type(n as u32);
    let table_init = entry_t.const_array(&entries);
    let table_g = llvm_module.add_global(table_t, None, "__torajs_class_layouts");
    table_g.set_initializer(&table_init);
    table_g.set_constant(true);
    table_g.set_linkage(inkwell::module::Linkage::External);
    let count_g = llvm_module.add_global(i32_t, None, "__torajs_n_class_layouts");
    count_g.set_initializer(&i32_t.const_int(n as u64, false));
    count_g.set_constant(true);
    count_g.set_linkage(inkwell::module::Linkage::External);
}
