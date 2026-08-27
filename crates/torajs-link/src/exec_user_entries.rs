//! User-supplied entry structs that appear in [`crate::exec::LinkConfig`]
//! before being chained into the link pipeline. Carved out of
//! `exec.rs` in W-J Phase A3c chunk 2 to keep `exec.rs` ≤ 500 prod
//! after the `class_names` + `force_emit_class_names_globals`
//! field-pair additions to `LinkConfig`. Re-exported by `exec.rs`
//! so every existing `crate::exec::User*Entry` call site keeps
//! working.

/// Two link-emitted user-string flavours (LLVM-era `ssa_inkwell::globals` ABI):
/// `StaticStr` = full rodata Str header+payload, `RawBytes` = payload-only
/// (runtime `__torajs_str_alloc` consumes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStringKind {
    StaticStr,
    RawBytes,
}

/// SD-4c-prereq+e0/e1/e2 — user-binary string literal payload. `sym`
/// is the codegen ADRP+ADD target (`__torajs_str_lit_*` /
/// `__torajs_str_dyn_*`); `kind` picks the emit ABI per
/// [`UserStringKind`].
#[derive(Debug, Clone)]
pub struct UserStringEntry {
    pub sym: String,
    /// Latin-1: 1 B / code unit; UTF-16: 2 B / unit LE.
    pub bytes: Vec<u8>,
    /// Drives `STR_FLAG_IS_LATIN1` (ignored when `kind = RawBytes`).
    pub is_latin1: bool,
    /// ES `String.length` code unit count.
    pub length: u32,
    pub kind: UserStringKind,
    /// A second sym the link registers at the entry's payload — the
    /// header's end, `vaddr + STR_HEADER_SIZE`. A literal's raw-byte
    /// readers (`InstKind::StringRef`, the fn-name and class-name
    /// tables) address `__torajs_str_dyn_<i>`; the bytes they want
    /// are exactly the ones already sitting behind `__torajs_str_lit_<i>`'s
    /// header, so the alias points into that entry instead of a
    /// second copy of the payload. `StaticStr` only.
    pub payload_alias: Option<String>,
}

/// SD-4c-prereq+e7 / r506 — one `[N x i64]` vtable. `sym` is what
/// codegen's `GlobalRef("__vtable_<C>")` ADRP+ADD targets; slots
/// resolve at emit time to `fn vaddr - table vaddr` (None → 0) and
/// the dispatch adds the table base back.
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
    /// ES-spec `Function.length` (chunk 716) — emitted into the
    /// entry's former `_pad: u32` slot.
    pub arity: u32,
    /// RFC 20260719-fn-tostring-source B3b — `__torajs_str_dyn_<sid>`
    /// alias for the type-erased fn source text (RawBytes flavour,
    /// same channel as `name_ptr_sym`). `None` emits a NULL
    /// `src_ptr` slot (no chain fixup) — synthesized decls with no
    /// user-written source.
    pub src_ptr_sym: Option<String>,
    /// ES `String.length` of the erased source; 0 when
    /// `src_ptr_sym` is `None`.
    pub src_len: u32,
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
    /// L3b #4 — declared class (`true`) vs anonymous struct shape
    /// (`false`). Written into the outer entry's flags word (bit 0)
    /// so the runtime Obj drop can restrict cycle-root buffering to
    /// named-class instances (mirrors the typed drop's policy).
    pub is_named: bool,
    /// 405-04 knife 2 fix — GENERIC specialization row (per-factory
    /// tag wearing the class identity, no registry slot of its own).
    /// Written into the outer entry's flags word (bit 1); the
    /// runtime proto/class alias resolver triggers only on it.
    pub is_generic: bool,
    /// 刀 4 (RFC 20260714-t262-top-clusters) — runtime-dispatchable
    /// methods (name + boxed-adapter fn). Lowered into the per-class
    /// `.__class_methods_<i>` inner global; the outer entry's
    /// `method_table_ptr` slot (+24) points at it (NULL when empty).
    pub methods: Vec<UserMethodMetaEntry>,
}

/// 刀 4 — one runtime-dispatchable method row. Mirrors
/// `ssa::MethodMetaSpec`; the adapter resolves through `fn_vaddrs`
/// by id at rebase-assembly time (the fn-name table's mechanism).
#[derive(Debug, Clone)]
pub struct UserMethodMetaEntry {
    /// Method name as declared (`next`, not `__cm_Gen__next`).
    pub name: String,
    /// The boxed adapter's `FuncId` index into the link layer's
    /// `fn_vaddrs` slice; `None` bakes a 0 adapter_ptr (r502 — the
    /// dead-strip pre-pass blanks the column when no runtime finder
    /// that invokes an adapter is live).
    pub adapter_fn_id: Option<u32>,
    /// S2.38 — MethodMeta flags word (bit 0 = this-free body); baked
    /// into the elem's formerly-pad u32 at +12.
    pub flags: u32,
    /// Blade 3 (RFC 20260804-method-rebind-generic-body) — the
    /// `__cmany_` twin's boxed adapter `FuncId` index, `None` when
    /// the method minted no twin (bakes a 0 twin_ptr slot).
    pub twin_fn_id: Option<u32>,
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

/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5a — one AOT-baked
/// regex per literal in the user binary. The link layer materialises
/// each entry as a `(BakedDfaMeta, [DfaState; N])` pair in
/// `__DATA_CONST` next to the vtable + class_layouts blobs; the
/// `__torajs_baked_regex_<index>` outer symbol points at the
/// `BakedDfaMeta` struct so the ssa_lower-emitted
/// `__torajs_regex_compile_from_static_dfa(meta_ptr, pat, flag)`
/// call has a vaddr to land on. Mirrors `ssa::BakedRegexEntry` minus
/// the SSA-side wiring — fields are byte-for-byte the same. Phase
/// C-5a defines the type + extends `LinkConfig` only; Phase C-5b
/// adds the `user_regex_baked_layout` module that consumes it; C-5c
/// hangs the inner `states_ptr` slot onto the chain-LC rebase chain.
#[derive(Debug, Clone)]
pub struct UserBakedRegexEntry {
    /// Position in `LinkConfig::baked_regex_entries` — surfaces in
    /// the emitted symbol name `__torajs_baked_regex_<index>`.
    pub index: u32,
    /// Raw byte image of `[DfaState; states_len]` per the
    /// `#[repr(C)] struct DfaState` ABI (1060 bytes per state on
    /// aarch64 — see `torajs_regex::dfa::DfaState`). ssa_lower
    /// (Phase C-6) serialises the host-built DFA into this buffer
    /// before pushing the entry.
    pub states_payload: Vec<u8>,
    /// `DfaState` count `states_payload` encodes. C-5b's emit
    /// pipeline asserts `states_payload.len() == states_len * 1060`.
    pub states_len: u32,
    /// Four anchored start state indices, mirroring
    /// `DfaProgram::{start, start_mid, start_mid_word,
    /// start_mid_nonword}`. Materialised into the `BakedDfaMeta`
    /// 32-byte struct verbatim.
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
    /// Round 3 Phase B attack #R-E — host-pre-computed mirror of
    /// `DfaProgram::any_accept_before_byte`. Emitted at offset 28 of
    /// the `BakedDfaMeta` (existing tail pad); `OUTER_META_SIZE`
    /// stays at 32 bytes.
    pub any_accept_before_byte: bool,
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
