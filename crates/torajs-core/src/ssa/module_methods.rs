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
    /// Interned string literals. StringId = index. Backend emits each as a
    /// global `[N x i8]` constant.
    pub strings: Vec<Vec<u8>>,
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
    /// T-24 — per-class virtual-method tables. ssa_inkwell emits each
    /// as a `[N x ptr]` LLVM constant global named `__vtable_<C>`,
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
    /// live. ssa_inkwell emits this as a runtime global so
    /// `runtime_cycle.c`'s visit_obj_children can drive a generic
    /// trial-deletion descent without needing per-class generated
    /// fns. Empty array => no class declared in the program (cycle
    /// collection is a no-op).
    pub class_layouts: Vec<ClassLayoutMeta>,
}

#[derive(Debug, Clone)]
pub struct DataGlobal {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct ClassLayoutMeta {
    /// Class name (informational; ssa_inkwell could use it to name
    /// a per-class debug symbol, but the runtime indexes by tag).
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

    pub fn intern_string(&mut self, bytes: Vec<u8>) -> StringId {
        let id = StringId(self.strings.len() as u32);
        self.strings.push(bytes);
        id
    }

    pub fn string_bytes(&self, id: StringId) -> &[u8] {
        &self.strings[id.0 as usize]
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

/// Hand-built fib(n: i64) -> i64 module — the same shape that
/// labs/0002-inkwell-spike emits as LLVM IR. Used by `tr ssa-demo` to
/// validate the IR types + pretty printer before the lowerer (step 2)
/// exists.
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
