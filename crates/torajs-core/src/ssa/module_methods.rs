//! `impl Module` (interning + pretty-print roots) and the
//! hand-built `demo_fib40` fixture.
//!
//! Extracted from `ssa.rs` (2026-05-25, god-file decomp batch 16).

use std::fmt::Write;

use super::{
    ArrId, BinOp, FuncId, Function, IPred, InstKind, Operand, SigId, StringId, StructId,
    Terminator, Type,
};

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

impl StringLiteral {
    /// Latin-1 wrapper for an already-encoded byte buffer where
    /// every byte is ≤ 0xFF (trivially true for `&[u8]`). Used by
    /// the obj-field-access fname matching path where the fname
    /// originates as an ASCII identifier `String` and the
    /// `StringRef` raw-byte consumer (`__torajs_str_eq_cstr`)
    /// needs the bytes verbatim.
    pub fn from_latin1_bytes(bytes: Vec<u8>) -> Self {
        let length = bytes.len() as u32;
        Self {
            bytes,
            is_latin1: true,
            length,
        }
    }

    /// P11.1-S2-a — build-time encoding decision for a string
    /// literal source string.
    ///
    /// Scans the source string's codepoints once. If every
    /// codepoint fits in Latin-1 (≤ 0xFF) the payload is encoded
    /// as one byte per code unit, matching the pre-S1 byte-Str
    /// layout exactly for the ASCII subset every existing fixture
    /// uses. Otherwise the payload is encoded as UTF-16 little-
    /// endian; codepoints in the BMP get a single u16 unit,
    /// codepoints in the supplementary planes (> 0xFFFF) split
    /// into a surrogate pair (high U+D800-U+DBFF + low
    /// U+DC00-U+DFFF) so `.length` matches ES `String.length` and
    /// `.charCodeAt(i)` indexes by code unit per spec §6.1.4.
    ///
    /// Mirrors V8 `String::Flatten`'s OneByte / TwoByte encoding
    /// split and SpiderMonkey `JSLinearString` Latin-1 / TwoByte
    /// representation, but lifted to AOT compile time — no
    /// startup atomization pass needed (the literal table is
    /// materialized directly into `.rodata`).
    pub fn encode_from_str(s: &str) -> Self {
        // Single pass: track max codepoint to pick encoding
        // without a second walk. `chars()` already decodes UTF-8
        // → Unicode scalar values so codepoints stay well-formed
        // up front.
        let mut max_cp: u32 = 0;
        for c in s.chars() {
            let cp = c as u32;
            if cp > max_cp {
                max_cp = cp;
            }
        }
        let is_latin1 = max_cp <= 0xFF;
        if is_latin1 {
            // Every codepoint fits in one byte; re-encode as
            // Latin-1 (one byte per code unit, identical to the
            // codepoint numeric value). For pure ASCII payloads
            // this is also identical to the source UTF-8 bytes —
            // every existing fixture lands here and stays
            // byte-identical.
            let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
            let length = bytes.len() as u32;
            Self {
                bytes,
                is_latin1: true,
                length,
            }
        } else {
            let mut bytes: Vec<u8> = Vec::with_capacity(s.len() * 2);
            let mut length: u32 = 0;
            for c in s.chars() {
                let cp = c as u32;
                if cp <= 0xFFFF {
                    // BMP — single u16 code unit, little-endian.
                    bytes.extend_from_slice(&(cp as u16).to_le_bytes());
                    length += 1;
                } else {
                    // Supplementary plane (> 0xFFFF, ≤ 0x10FFFF
                    // per Unicode). Surrogate pair encode per
                    // UTF-16 spec:
                    //   cp' = cp - 0x10000  (0..0xFFFFF, 20 bits)
                    //   hi  = 0xD800 | (cp' >> 10)  (top 10 bits)
                    //   lo  = 0xDC00 | (cp' & 0x3FF) (bottom 10)
                    let cp_off = cp - 0x10000;
                    let hi = 0xD800 | ((cp_off >> 10) as u16);
                    let lo = 0xDC00 | ((cp_off & 0x3FF) as u16);
                    bytes.extend_from_slice(&hi.to_le_bytes());
                    bytes.extend_from_slice(&lo.to_le_bytes());
                    length += 2; // surrogate pair = 2 code units
                }
            }
            Self {
                bytes,
                is_latin1: false,
                length,
            }
        }
    }
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

#[derive(Debug, Clone)]
pub struct ClassLayoutMeta {
    /// Class name (informational; useful for naming a per-class
    /// debug symbol, but the runtime indexes by tag).
    pub class_name: String,
    /// Byte offsets within an instance where refcounted heap-pointer
    /// fields live (already includes OBJ_HEADER_SIZE = 24). Used by
    /// the cycle collector's per-tag visitor to enumerate children
    /// during mark/scan/collect.
    pub child_offsets: Vec<u32>,
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

/// Hand-built fib(n: i64) -> i64 module — the same shape the retired
/// LLVM-gate spike (labs/0002, removed with the inkwell backend)
/// emitted as LLVM IR. Used by `tr ssa-demo` to validate the IR types
/// + pretty printer before the lowerer (step 2) existed.
pub fn demo_fib40() -> Module {
    let mut m = Module::default();
    let mut fib = Function::new("fib", Type::I64);
    let n = fib.add_param(Type::I64, "n");
    let bb_entry = fib.add_block();
    let bb_base = fib.add_block();
    let bb_recurse = fib.add_block();

    // bb_entry:  %t = icmp slt %n, 2;  cond_br %t, bb_base, bb_recurse
    let t = fib.append_inst(
        bb_entry,
        InstKind::ICmp(IPred::Slt, Operand::Value(n), Operand::ConstI64(2)),
        Type::Bool,
        Some("t"),
    );
    fib.set_term(
        bb_entry,
        Terminator::CondBr {
            cond: Operand::Value(t),
            then_blk: bb_base,
            else_blk: bb_recurse,
        },
    );

    // bb_base:   ret %n
    fib.set_term(bb_base, Terminator::Ret(Some(Operand::Value(n))));

    // bb_recurse: %a = sub %n, 1
    //             %r1 = call fib(%a)
    //             %b = sub %n, 2
    //             %r2 = call fib(%b)
    //             %s = add %r1, %r2
    //             ret %s
    let a = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Sub, Operand::Value(n), Operand::ConstI64(1)),
        Type::I64,
        Some("a"),
    );
    let fib_id = FuncId(0); // first function in this module
    let r1 = fib.append_inst(
        bb_recurse,
        InstKind::Call(fib_id, vec![Operand::Value(a)]),
        Type::I64,
        Some("r1"),
    );
    let b = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Sub, Operand::Value(n), Operand::ConstI64(2)),
        Type::I64,
        Some("b"),
    );
    let r2 = fib.append_inst(
        bb_recurse,
        InstKind::Call(fib_id, vec![Operand::Value(b)]),
        Type::I64,
        Some("r2"),
    );
    let s = fib.append_inst(
        bb_recurse,
        InstKind::BinOp(BinOp::Add, Operand::Value(r1), Operand::Value(r2)),
        Type::I64,
        Some("s"),
    );
    fib.set_term(bb_recurse, Terminator::Ret(Some(Operand::Value(s))));

    m.add_function(fib);
    m
}
