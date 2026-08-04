//! `impl Module` (interning + pretty-print roots) and the SSA-side
//! type / global definitions (`Module` / `StringLiteral` / `DataGlobal`
//! / `ClassLayoutMeta` / `VtableGlobal` / etc). `impl StringLiteral`
//! and `demo_fib40` live in the sibling `module_extras.rs`.
//!
//! Extracted from `ssa.rs` (2026-05-25, god-file decomp batch 16).

use std::fmt::Write;

use super::module_class_layouts::ClassLayoutMeta;
use super::{ArrId, FuncId, Function, SigId, StringId, StructId, Type};

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub funcs: Vec<Function>,
    /// Interned string literals. StringId = index. Backend emits each
    /// as a raw-byte `[N x i8]` global (consumed by `InstKind::StringRef`)
    /// and a Str-shaped global with header / length / encoding flag bit
    /// (consumed by `InstKind::StaticStrRef`). Each entry carries the
    /// encoded payload bytes plus the encoding metadata the
    /// `emit_static_str_global` writer needs to lay out the rodata
    /// header correctly.
    pub strings: Vec<StringLiteral>,
    /// Interned struct layouts — `Vec<(field_name, field_type)>`. Field
    /// order matters (it's the layout). Two structurally-equal types
    /// share a single StructId via `intern_struct`. Layouts can recurse
    /// (a struct field of type `Obj(_)` references back into this Vec).
    pub struct_layouts: Vec<Vec<(String, Type)>>,
    /// Interned `Array<T>` element types. ArrId = index. Two arrays of
    /// the same element type share one ArrId via `intern_arr`.
    pub arr_layouts: Vec<Type>,
    /// Interned fn-pointer signatures `(Vec<param_types>, ret_type)`.
    /// SigId = index. Used by `InstKind::CallIndirect` to look up the
    /// calling convention at codegen. M2 Phase B Stage 2.
    pub signatures: Vec<(Vec<Type>, Type)>,
    /// Phase K.3 — module-level data globals declared by top-level
    /// `let X: T = <init>`. The backend emits one LLVM global per
    /// entry (zero-initialized; the SSA `main` fn runs `<init>` and
    /// `Store`s the result into the slot before any other code). Reads
    /// from named-fn bodies lower to `GlobalRef(name)` + `Load(ty, ...)`;
    /// writes lower to `GlobalRef(name)` + `Store(value, ...)`.
    pub data_globals: Vec<DataGlobal>,
    /// T-24 — per-class virtual-method tables. torajs-link
    /// materializes each as a pointer-array global named `__vtable_<C>`,
    /// where slot[i] = the FuncId of `__cm_<best-owner-of-method[i]>__M`
    /// (or None if class C's MRO has no impl of method[i] — that slot
    /// becomes a null ptr that should never be loaded for this class).
    /// Class instances stamp the global's address into
    /// `OBJ_VTABLE_OFF (=16)` at construction time; `__dispatch_<M>`
    /// loads `vtable[method_index] -> fn_ptr` and `CallIndirect`s.
    pub vtable_globals: Vec<VtableGlobal>,
    /// T-26.C — per-class children-offset metadata for the cycle
    /// collector's mark/scan/collect walks. Indexed by `class_tag - 1`
    /// (tag 0 reserved for "not a class"); each entry lists the byte
    /// offsets within the obj where refcounted heap-pointer fields
    /// live. torajs-link materializes this as a runtime global so
    /// torajs-cycle's visit_obj_children can drive a generic
    /// trial-deletion descent without needing per-class generated
    /// fns. Empty array => no class declared in the program (cycle
    /// collection is a no-op).
    pub class_layouts: Vec<ClassLayoutMeta>,
    /// Function-address → declared-name registry. Populated by
    /// `ssa_lower` for every top-level `function foo()` declaration
    /// (and arrow / named function expressions with a knowable
    /// binding context). torajs-link materializes this as a sorted
    /// `__torajs_fn_name_table[]` rodata global so the runtime
    /// helper `__torajs_fn_print_inline(fn_addr)` (see
    /// torajs-anyvalue::inspect Tag::Closure arm + the SSA
    /// dispatcher's Type::FnSig case) can do a binary search and
    /// emit the bun `[Function: name]` form instead of the raw
    /// pointer fallthrough. fn-addr resolution happens at link time
    /// via the same chain-fixup pipeline `user_vtables_layout`
    /// uses, so ASLR slide is honoured on macOS PIE binaries.
    pub fn_name_globals: Vec<FnNameEntry>,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-4 — per-literal-RegExp
    /// AOT-baked DFA blob. ssa_lower's `Expr::Regex` arm pushes one
    /// entry per DFA-eligible literal regex (`/abc/`, `/foo\d+/g`,
    /// ...); torajs-link's `user_regex_baked_layout` (Phase C-5)
    /// materialises each as a `.rodata` `BakedDfaMeta` + `[DfaState; N]`
    /// pair in `__DATA_CONST` next to the vtable + class_layouts
    /// blobs (chain-LC rebased for the inner `states_ptr`). The user
    /// binary calls `__torajs_regex_compile_from_static_dfa(meta_ptr,
    /// pat, flag)` instead of `__torajs_regex_compile` for these
    /// literals, so the runtime path skips `build_dfa` AND
    /// sidesteps the `UnsafeCell<Option<DfaProgram>>` borrow shape
    /// that produced the chunk-7.6 / chunk-7.7 SIGBUS — see
    /// `RegExp::baked_dfa_view` for the surface-side reader.
    ///
    /// Empty when no DFA-eligible literal regex appears in the
    /// program; ineligible regex literals + `new RegExp(...)`
    /// dynamic constructs continue routing through `regex_compile`.
    pub baked_regex_entries: Vec<BakedRegexEntry>,
}

#[derive(Debug, Clone)]
pub struct FnNameEntry {
    /// Which fn this entry names. The link-time emitter resolves
    /// this to the fn's final vaddr through the standard symbol
    /// table + chain-fixup path.
    pub fn_id: FuncId,
    /// Surface name as the user wrote it. For top-level `function foo()`
    /// declarations this is `"foo"`; for `let f = () => {}` arrow
    /// expressions assigned to a single binding context, it's the
    /// binding name `"f"`; anonymous closures don't get an entry
    /// (runtime falls back to `[Function (anonymous)]`).
    pub name: String,
    /// String table id where the name's raw byte payload lives —
    /// `Module::strings[name_sid.0]`. The link layer turns this into
    /// the `__user_string_<sid>` alias for the rodata table's
    /// name_ptr chain-fixup target.
    pub name_sid: StringId,
    /// ES-spec `Function.length` — the count of leading params
    /// before the first default / rest param (chunk 716). Rides the
    /// rodata entry's former `_pad: u32` slot so the runtime
    /// `.length` read answers it without an ABI size change.
    pub arity: u32,
    /// RFC 20260719-fn-tostring-source B3b — string table id of the
    /// type-erased source text (`fn_source_erase::erase_types` over
    /// the recorded decl span), or `None` when the decl carries the
    /// (0,0) sentinel span (synthesized forwarders / bound wrappers).
    /// The link layer bakes it as the entry's `src_ptr` chain-fixup
    /// target; `None` emits a NULL src_ptr the runtime treats as
    /// "no source recorded".
    pub src_sid: Option<StringId>,
    /// ES `String.length` of the erased source (code units — same
    /// contract as the interned literal's `length`). 0 when
    /// `src_sid` is `None`.
    pub src_len: u32,
}

#[derive(Debug, Clone)]
pub struct DataGlobal {
    pub name: String,
    pub ty: Type,
}

/// P11.1-S2-a — interned string literal payload + encoding metadata.
///
/// `bytes` is the raw encoded payload (Latin-1: one byte per code
/// unit; UTF-16: two little-endian bytes per code unit). `length`
/// is the code unit count per ES `String.length` (Latin-1: =
/// bytes.len(); UTF-16: = bytes.len() / 2). `is_latin1` discriminates
/// the encoding so `emit_static_str_global` can set the
/// `STR_FLAG_IS_LATIN1` bit on the rodata Str's header flags.
///
/// Encoding decision is made at parse → SSA lowering time by
/// scanning the source string's codepoints once
/// ([`crate::ssa_lower::LowerCtx::intern_string_literal`]): max
/// codepoint ≤ 0xFF → Latin-1; otherwise UTF-16 with surrogate
/// pair encoding for codepoints > 0xFFFF.
#[derive(Debug, Clone)]
pub struct StringLiteral {
    pub bytes: Vec<u8>,
    pub is_latin1: bool,
    pub length: u32,
}

#[cfg(test)]
mod string_literal_tests {
    use super::StringLiteral;

    #[test]
    fn ascii_encodes_as_latin1_byte_identical() {
        let lit = StringLiteral::encode_from_str("abc");
        assert!(lit.is_latin1);
        assert_eq!(lit.bytes, b"abc");
        assert_eq!(lit.length, 3);
    }

    #[test]
    fn latin1_supplement_stays_latin1() {
        // U+00E9 ('é') — Latin-1 supplement, ≤ 0xFF, so still
        // Latin-1 encoded as one byte (0xE9).
        let lit = StringLiteral::encode_from_str("caf\u{00E9}");
        assert!(lit.is_latin1);
        assert_eq!(lit.bytes, [b'c', b'a', b'f', 0xE9]);
        assert_eq!(lit.length, 4);
    }

    #[test]
    fn bmp_above_0xff_encodes_as_utf16_le() {
        // U+4E2D ('中') — BMP, > 0xFF → UTF-16 LE, 2 bytes per
        // code unit.
        let lit = StringLiteral::encode_from_str("\u{4E2D}\u{6587}");
        assert!(!lit.is_latin1);
        assert_eq!(lit.bytes, [0x2D, 0x4E, 0x87, 0x65]);
        assert_eq!(lit.length, 2);
    }

    #[test]
    fn supplementary_plane_encodes_as_surrogate_pair() {
        // U+1F600 ('😀') — supplementary plane.
        // cp - 0x10000 = 0xF600
        // hi = 0xD800 | (0xF600 >> 10) = 0xD800 | 0x3D = 0xD83D
        // lo = 0xDC00 | (0xF600 & 0x3FF) = 0xDC00 | 0x200 = 0xDE00
        let lit = StringLiteral::encode_from_str("\u{1F600}");
        assert!(!lit.is_latin1);
        assert_eq!(lit.bytes, [0x3D, 0xD8, 0x00, 0xDE]);
        assert_eq!(lit.length, 2);
    }

    #[test]
    fn mixed_bmp_and_surrogate_pair() {
        // "A😀" — 'A' is ASCII so any non-ASCII codepoint forces
        // the whole string to UTF-16. The 'A' becomes a single
        // BMP code unit (0x0041 → "\x41\x00"), the emoji becomes
        // a surrogate pair (0xD83D 0xDE00).
        let lit = StringLiteral::encode_from_str("A\u{1F600}");
        assert!(!lit.is_latin1);
        assert_eq!(lit.bytes, [0x41, 0x00, 0x3D, 0xD8, 0x00, 0xDE]);
        assert_eq!(lit.length, 3); // 'A' (1) + surrogate pair (2)
    }

    #[test]
    fn from_latin1_bytes_helper_matches_encode_for_ascii() {
        let a = StringLiteral::from_latin1_bytes(b"hello".to_vec());
        let b = StringLiteral::encode_from_str("hello");
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.length, b.length);
        assert_eq!(a.is_latin1, b.is_latin1);
    }
}

/// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-4 — one AOT-baked DFA
/// per literal RegExp. ssa_lower allocates one entry per
/// DFA-eligible literal in [`Module::baked_regex_entries`];
/// torajs-link's `user_regex_baked_layout` (Phase C-5) reads these to
/// emit the `.rodata` `BakedDfaMeta` struct + the `[DfaState; N]`
/// payload as a chain-LC-rebase-aware pair.
///
/// `index` is the position in `Module::baked_regex_entries` —
/// torajs-link uses it to compose the per-entry symbol name
/// (e.g. `__torajs_baked_regex_<index>`). `states_payload` is the
/// raw byte image of the `[DfaState; N]` table that ssa_lower
/// host-built via `torajs_regex::dfa::build_dfa`, **already serialised
/// to the `#[repr(C)]` DfaState byte layout** (1060 bytes per state;
/// see `BakedDfaMeta`'s sibling doc on `DfaState`). The four start
/// indices match `DfaProgram::{start, start_mid, start_mid_word,
/// start_mid_nonword}` so the runtime view's lookup arms see the
/// same anchored entries.
#[derive(Debug, Clone)]
pub struct BakedRegexEntry {
    /// Position in [`Module::baked_regex_entries`]; surfaces in the
    /// emitted symbol name. Stable across this build's lifetime;
    /// changing entry count invalidates downstream link state, so the
    /// emit pipeline always re-runs after a `baked_regex_entries`
    /// edit.
    pub index: u32,
    /// Raw byte image of `[DfaState; states_len]` laid out per the
    /// `#[repr(C)] struct DfaState` ABI documented at
    /// `torajs_regex::dfa::DfaState`. ssa_lower must populate this
    /// by host-building the DFA + serialising each `DfaState`
    /// `#[repr(C)]`-byte-by-byte; the runtime side reads it via
    /// `from_raw_parts(states_ptr, states_len)`.
    pub states_payload: Vec<u8>,
    /// Number of `DfaState` entries `states_payload` encodes.
    /// `states_payload.len() == states_len * 1060` on aarch64
    /// (sized check belongs in the emit pipeline, not on this
    /// struct).
    pub states_len: u32,
    /// Four anchored start state indices — match
    /// `DfaProgram::{start, start_mid, start_mid_word,
    /// start_mid_nonword}`.
    pub start: u32,
    pub start_mid: u32,
    pub start_mid_word: u32,
    pub start_mid_nonword: u32,
    /// Round 3 Phase B attack #R-E — host-pre-computed mirror of
    /// `DfaProgram::any_accept_before_byte`. Lands at offset 28 in
    /// the emitted `BakedDfaMeta` 32-byte struct (existing tail pad);
    /// `OUTER_META_SIZE` stays at 32 bytes.
    pub any_accept_before_byte: bool,
}

#[derive(Debug, Clone)]
pub struct VtableGlobal {
    /// Surface-level class name (`"Animal"`, `"Promise"`, etc.). The
    /// emitted LLVM symbol is `__vtable_<class_name>`.
    pub class_name: String,
    /// Slot[i] = the `__cm_<X>__<method[i]>` fn for whichever class
    /// X is the deepest ancestor of `class_name` (incl. itself) that
    /// has an own impl. None = no impl in MRO; the slot is null.
    /// Length matches `ast.method_index`'s entry count.
    pub fn_ids: Vec<Option<FuncId>>,
}

impl Module {
    pub fn add_function(&mut self, f: Function) -> FuncId {
        let id = FuncId(self.funcs.len() as u32);
        self.funcs.push(f);
        id
    }

    pub fn func_name(&self, id: FuncId) -> &str {
        &self.funcs[id.0 as usize].name
    }

    /// Intern a raw byte buffer as a Latin-1 string literal. Used by
    /// demo / test fixtures + callers that already hold an
    /// encoded byte payload; full encoding decisions go through
    /// `ssa_lower::encode_literal` instead.
    pub fn intern_string(&mut self, bytes: Vec<u8>) -> StringId {
        let id = StringId(self.strings.len() as u32);
        self.strings.push(StringLiteral::from_latin1_bytes(bytes));
        id
    }

    pub fn string_bytes(&self, id: StringId) -> &[u8] {
        &self.strings[id.0 as usize].bytes
    }

    /// Intern a struct layout. Returns an existing StructId if a
    /// structurally-equal layout was already registered, else allocates
    /// a fresh one. Field-name order matters — `{x, y}` ≠ `{y, x}`.
    pub fn intern_struct(&mut self, layout: Vec<(String, Type)>) -> StructId {
        for (i, existing) in self.struct_layouts.iter().enumerate() {
            if *existing == layout {
                return StructId(i as u32);
            }
        }
        let id = StructId(self.struct_layouts.len() as u32);
        self.struct_layouts.push(layout);
        id
    }

    pub fn struct_layout(&self, id: StructId) -> &[(String, Type)] {
        &self.struct_layouts[id.0 as usize]
    }

    /// Byte size of a struct, given the MVP's flat 8-byte-per-field rule.
    /// (P2.4.c restriction: only Copy fields, all stored in 8-byte slots
    /// regardless of actual field type. P2.4.d will reduce padding for
    /// smaller types.)
    pub fn struct_size(&self, id: StructId) -> u64 {
        self.struct_layout(id).len() as u64 * 8
    }

    /// Intern an `Array<T>` element type. Returns the existing ArrId if
    /// the same element type was already registered.
    pub fn intern_arr(&mut self, elem: Type) -> ArrId {
        for (i, existing) in self.arr_layouts.iter().enumerate() {
            if *existing == elem {
                return ArrId(i as u32);
            }
        }
        let id = ArrId(self.arr_layouts.len() as u32);
        self.arr_layouts.push(elem);
        id
    }

    pub fn arr_elem(&self, id: ArrId) -> Type {
        self.arr_layouts[id.0 as usize]
    }

    /// Intern a fn-pointer signature. M2 Phase B Stage 2.
    pub fn intern_signature(&mut self, params: Vec<Type>, ret: Type) -> SigId {
        for (i, existing) in self.signatures.iter().enumerate() {
            if existing.0 == params && existing.1 == ret {
                return SigId(i as u32);
            }
        }
        let id = SigId(self.signatures.len() as u32);
        self.signatures.push((params, ret));
        id
    }

    pub fn signature(&self, id: SigId) -> &(Vec<Type>, Type) {
        &self.signatures[id.0 as usize]
    }

    /// Pretty-print to stdout. Format is intentionally LLVM-IR-shaped so a
    /// reader who knows LLVM IR can read this without a guide.
    pub fn print(&self) {
        let mut buf = String::new();
        self.write_to(&mut buf).unwrap();
        print!("{buf}");
    }

    pub fn write_to(&self, w: &mut String) -> std::fmt::Result {
        for (i, f) in self.funcs.iter().enumerate() {
            if i > 0 {
                writeln!(w)?;
            }
            f.write_to(w, self)?;
        }
        Ok(())
    }
}
