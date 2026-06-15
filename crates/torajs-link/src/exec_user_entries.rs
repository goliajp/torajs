//! User-supplied entry structs that appear in [`crate::exec::LinkConfig`]
//! before being chained into the link pipeline. Carved out of
//! `exec.rs` in W-J Phase A3c chunk 2 to keep `exec.rs` ≤ 500 prod
//! after the `class_names` + `force_emit_class_names_globals`
//! field-pair additions to `LinkConfig`. Re-exported by `exec.rs`
//! so every existing `crate::exec::User*Entry` call site keeps
//! working.

/// SD-4c-prereq+e7 — one `[N x ptr]` vtable. `sym` is what codegen's
/// `GlobalRef("__vtable_<C>")` ADRP+ADD targets; slots resolve at
/// emit time (Some(name) → name's final vaddr; None → 0).
#[derive(Debug, Clone)]
pub struct UserVtableEntry {
    pub sym: String,
    pub slot_syms: Vec<Option<String>>,
}

/// Fn-name registry Phase 2 Step 3b.2 — one row of the
/// `__torajs_fn_name_table[]` rodata array. The link layer pairs
/// each fn body with its source-text name so the runtime
/// `__torajs_fn_print_inline` can binary-search a fn_addr into a
/// `[Function: <name>]` print.
///
/// `fn_addr_sym` is `__torajs_fn_<fid>` — the same alias
/// `register_fn_addr_syms` materializes from `fn_vaddrs`. Resolves
/// to the user fn's body vaddr at link time.
///
/// `name_ptr_sym` is `__torajs_str_dyn_<sid>` — the RawBytes
/// flavour `apply_user_string_overrides` registers for an interned
/// string literal. Points at the raw char payload (no 16-byte Str
/// header) so the runtime helper can `putc` each byte directly.
///
/// `name_len` is the source-text code-unit count per ES
/// `String.length` — written as a literal `u32` next to the two
/// chain-fixup-encoded `u64`s in the rodata entry.
#[derive(Debug, Clone)]
pub struct UserFnNameEntry {
    pub fn_addr_sym: String,
    pub name_ptr_sym: String,
    pub name_len: u32,
}

/// W-J Phase A3c — one row of the `__torajs_class_name_table[]`
/// rodata array. The link layer pairs each registered class with
/// its source-text name so the runtime
/// `__torajs_struct_class_name(class_tag)` can binary-search a
/// tag into the bun `[name] {…}` print prefix.
#[derive(Debug, Clone)]
pub struct UserClassNameEntry {
    /// SSA-assigned class_tag (1-based; 0 reserved for anonymous /
    /// non-stamped). Written as a literal `u32` at entry offset 0;
    /// the runtime helper binary-searches on this key.
    pub class_tag: u32,
    /// `__torajs_str_dyn_<sid>` alias sym — the RawBytes flavour
    /// `apply_user_string_overrides` registers for an interned
    /// class-name string. Resolves at link time to the raw char
    /// payload's vaddr (no 16-byte Str header). cmd_build.rs's
    /// SSA bridge interns each `ssa::ClassLayoutMeta.class_name`
    /// into `LinkConfig.strings` and records the resulting sid here.
    pub name_ptr_sym: String,
    /// Code-unit count per ES `String.length`. Lives in the
    /// `name_len: u32` field (NOT a chain-fixup target).
    pub name_len: u32,
}

/// e8 + W-J A3b — per-class `child_offsets` for the cycle collector
/// (T-26.C) + per-field name/offset/type_tag metadata for the reflection
/// consumers (Phase B gOPD struct arm / Phase C Object.keys/values/entries
/// / Phase D inspect.rs Tag::Obj walker). Mirrors `ssa::ClassLayoutMeta`
/// minus `class_name`.
#[derive(Debug, Clone)]
pub struct UserClassLayoutEntry {
    /// Byte offsets of refcounted heap-ptr fields (post `OBJ_HEADER_SIZE`).
    pub child_offsets: Vec<u32>,
    /// W-J A3b — per-field metadata (one entry per struct field, in
    /// declaration order). Empty Vec means no metadata available; the
    /// outer entry's `field_metadata_ptr` slot stays NULL and the
    /// reflection helper short-circuits.
    pub fields: Vec<UserFieldMetaEntry>,
}

/// W-J A3b — one per-field metadata row carried into the link layer.
/// Mirrors `ssa::FieldMetaSpec`; lowered into the per-class
/// `.__class_fields_<i>` inner global at link time.
#[derive(Debug, Clone)]
pub struct UserFieldMetaEntry {
    /// Field name as it appears in the struct literal / class decl.
    pub name: String,
    /// Byte offset within the instance (= `OBJ_HEADER_SIZE + i*8`).
    pub offset: u32,
    /// Coarse 8-bit `Type` discriminator (`ssa::field_type_tag_of`).
    pub type_tag: u8,
}

/// e4 — user-binary top-level mutable global slot. `__DATA,__bss`
/// zerofill (loader supplies zeros). `size`/`align_log2` from SSA `Type`.
#[derive(Debug, Clone)]
pub struct UserDataGlobalEntry {
    /// Codegen `Page21/PageOff12` target — matches `DataGlobal.name`.
    pub sym: String,
    /// I64/F64/ptr=8, I32=4, Bool=1.
    pub size: u32,
    /// 0=byte, 2=u32, 3=u64.
    pub align_log2: u8,
}
