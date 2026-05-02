#![allow(dead_code)] // step 1: types only; lowerer (step 2) + backend (step 3) consume the rest

// SSA IR for the new codegen path (P3.5).
//
// This is the IR that frontend (lex/parse/check) lowers into, and that the
// LLVM backend (P3.5+) and Cranelift backend (P3.6) both consume. It exists
// alongside the stack-machine `ir.rs` (which feeds the tree-walk interpreter
// and is on the retirement list with the wasm-via-C path).
//
// Step 1 of P3.5: define the types + pretty printer + a hand-built fib40
// demo that round-trips through `tr ssa-demo`. The lowerer (AST → SSA) is
// step 2; the LLVM backend (SSA → Inkwell) is step 3.
//
// Design notes:
// - **Operands carry constants inline** (Operand::ConstI64 etc.) rather than
//   going through their own SSA value. Matches LLVM IR's actual textual
//   shape and keeps the pretty-printed output readable.
// - **Newtype IDs** for ValueId/BlockId/FuncId — cheap type safety, harder
//   to confuse a value index with a block index.
// - **Per-function value table** holds the type and optional debug name of
//   each ValueId. Optional name is what makes `%n` / `%t` / `%r1` show up
//   in the pretty output instead of `%0` / `%4` / `%7`.
// - **No phi nodes yet** — fib40 only needs branching, not loop carry. Phis
//   will land in step 2 when we lower `while`.

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringId(pub u32);

/// Index into `Module.struct_layouts`. Two `StructId`s compare equal iff
/// they refer to the same interned layout (i.e. structurally equal types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub u32);

/// Index into `Module.arr_layouts`. Each entry holds one `Array<T>`
/// instantiation's element type. Two `ArrId`s compare equal iff they
/// refer to the same element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArrId(pub u32);

/// Index into `Module.signatures`. Each entry holds one fn-pointer
/// signature `(Vec<param_types>, ret_type)`. Two `SigId`s compare equal
/// iff their signatures are identical. M2 Phase B Stage 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I64,
    F64,
    I32,
    Bool,
    Void,
    /// LLVM 22 uses opaque pointers — no need to track what's pointed at.
    /// The Load instruction carries the loaded type explicitly; Store
    /// derives it from the value operand's type.
    Ptr,
    /// Owned heap-string handle. At codegen this lowers to the same
    /// machine type as Ptr (a single pointer), but at the SSA layer it
    /// stays distinct from a generic alloca pointer so that:
    ///   - `console.log(s)` can dispatch to print_str vs print_i64 by
    ///     reading the operand's SSA type
    ///   - drop emission (P2.2.b) knows which slots need free()
    ///   - future inline-small-string layout can change the codegen
    ///     without touching the SSA shape
    /// Step 2.2.a: only static-pointer (literal) backed strings — the
    /// pointer is a `[N x i8]` global; drop is a no-op. Concat + true
    /// heap allocation lands in 2.2.b/c.
    Str,
    /// Substring view — non-owning slice of an owned `Str`. Layout:
    /// `[header:8][len:8][parent_ptr:8][offset:8]` (32 bytes). The
    /// view holds a refcount on its parent Str so the source bytes
    /// stay alive; view's drop dec's parent's refcount before free.
    ///
    /// Created by `s.split(sep)`, `s.slice(start, end)` etc. when the
    /// result is a borrow into the source's bytes (zero `memcpy`,
    /// zero per-substring byte alloc). Mirrors Swift's `Substring` /
    /// Rust's `&str` slice — separate type from `Str` so the OWNED
    /// hot-path doesn't pay any indirection cost. At codegen this
    /// also lowers to a single pointer (same as `Str`), but the SSA
    /// distinction routes `.charCodeAt` / `=== "literal"` / etc. to
    /// view-aware variants that load bytes via `parent_data + offset`
    /// instead of `self + 16`.
    ///
    /// Type system: TS source has no separate syntax for substring
    /// (only `string`); the compiler infers `Substr` for split / slice
    /// outputs and propagates it through let-binds + for-of. At fn-
    /// call boundaries that expect `Str`, the call site auto-coerces
    /// (Phase Substr.B materializes; Phase Substr.C will mono-
    /// morphize the callee for both Str and Substr arg types to keep
    /// view performance across boundaries).
    Substr,
    /// Owned heap object handle pointing at a struct with the layout
    /// stored at `module.struct_layouts[id]`. Like Str, lowers to a
    /// single pointer at codegen — the SSA-level distinction lets
    /// drop emission look up which fields are non-Copy and (in P2.4.d)
    /// recursively drop them before freeing the outer struct.
    /// P2.4.c MVP: layout is N×8-byte slots in field declaration order;
    /// only Copy fields supported (recursive drop comes in P2.4.d).
    Obj(StructId),
    /// Owned heap array of `T`. Layout: `{u64 len, u64 cap, T data[cap]}`
    /// with uniform 8-byte slots regardless of element type — primitives
    /// store directly, heap-typed elements (Str / Obj / nested Arr)
    /// store a pointer. M1.2 MVP. The element type interns into
    /// `module.arr_layouts[id]`.
    Arr(ArrId),
    /// Function-pointer value, typed by interned signature. Lowers to
    /// pointer-width at codegen; the signature info routes indirect
    /// calls (`InstKind::CallIndirect`) so backends can build the
    /// right calling convention. M2 Phase B Stage 2.
    FnSig(SigId),
    /// Closure value — a heap pointer to an env block whose layout is
    /// `[i64 fn_ptr, capture_0, capture_1, ...]`. SigId is the
    /// **user-visible** signature (without the env first param).
    /// Codegen lowers to a single pointer; calling a closure loads the
    /// fn pointer from env+0 and indirect-calls with env as the first
    /// argument. Heap-owned, non-Copy (the env block is freed when the
    /// last owner of the closure binding goes out of scope).
    Closure(SigId),
}

impl Type {
    pub fn as_str(self) -> &'static str {
        match self {
            Type::I64 => "i64",
            Type::F64 => "f64",
            Type::I32 => "i32",
            Type::Bool => "bool",
            Type::Void => "void",
            Type::Ptr => "ptr",
            Type::Str => "str",
            Type::Substr => "substr",
            Type::Obj(_) => "obj",
            Type::Arr(_) => "arr",
            Type::FnSig(_) => "fnsig",
            Type::Closure(_) => "closure",
        }
    }

    /// Cheap-to-duplicate. Used by the lowerer to decide whether a binding
    /// read needs ownership tracking + Drop emission. Mirrors check.rs's
    /// `Type::is_copy()`. Today only `Str` is heap-owned at the SSA layer;
    /// arrays / objects join the non-Copy side as they land.
    pub fn is_copy(self) -> bool {
        matches!(
            self,
            Type::I64
                | Type::F64
                | Type::I32
                | Type::Bool
                | Type::Void
                | Type::FnSig(_)
        )
        // Str + Obj + Arr are heap-owned, affine.
        // FnSig is just a fn pointer — Copy semantics, no drop.
        // Closure is heap-owned (env block) — non-Copy.
    }

    /// Phase B refcount: returns true if the heap object for this type
    /// begins with `__torajs_heap_header_t` (refcount@0, type_tag@4,
    /// flags@6). `__torajs_rc_inc` / `__torajs_rc_dec` are only safe
    /// to call on values of refcount-aware types.
    ///
    /// Phase 1: `Str`. Phase 2A: `Arr`. Phase 2B: `Obj`. Phase 2C:
    /// `Closure`. Phase Substr.A: `Substr` (also uses universal heap
    /// header; drop is view-aware — dec parent before free).
    pub fn is_refcounted(self) -> bool {
        matches!(
            self,
            Type::Str | Type::Substr | Type::Arr(_) | Type::Obj(_) | Type::Closure(_)
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Operand {
    Value(ValueId),
    ConstI64(i64),
    /// i32 constants only ever come up as `main`'s `ret 0` for now.
    ConstI32(i32),
    ConstF64(f64),
    ConstBool(bool),
    /// `null` literal value for a pointer-shaped slot (Str / Obj / Arr /
    /// Closure / FnSig). At codegen we emit `ptr_t.const_null()` —
    /// exactly the in-band 0 sentinel JS treats as nullish. Cheaper
    /// than ConstI64(0) since no inttoptr is needed.
    ConstPtrNull,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    // Integer
    Add,
    Sub,
    Mul,
    SDiv,
    SRem,
    And,
    Or,
    Xor,
    Shl,
    AShr, // arithmetic (signed) shift right
    LShr, // logical shift right
    // Floating point
    FAdd,
    FSub,
    FMul,
    FDiv,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::SDiv => "sdiv",
            BinOp::SRem => "srem",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::Shl => "shl",
            BinOp::AShr => "ashr",
            BinOp::LShr => "lshr",
            BinOp::FAdd => "fadd",
            BinOp::FSub => "fsub",
            BinOp::FMul => "fmul",
            BinOp::FDiv => "fdiv",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IPred {
    Eq,
    Ne,
    Slt,
    Sgt,
    Sle,
    Sge,
}

impl IPred {
    pub fn as_str(self) -> &'static str {
        match self {
            IPred::Eq => "eq",
            IPred::Ne => "ne",
            IPred::Slt => "slt",
            IPred::Sgt => "sgt",
            IPred::Sle => "sle",
            IPred::Sge => "sge",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FPred {
    Oeq,
    One,
    Olt,
    Ogt,
    Ole,
    Oge,
}

impl FPred {
    pub fn as_str(self) -> &'static str {
        match self {
            FPred::Oeq => "oeq",
            FPred::One => "one",
            FPred::Olt => "olt",
            FPred::Ogt => "ogt",
            FPred::Ole => "ole",
            FPred::Oge => "oge",
        }
    }
}

#[derive(Debug, Clone)]
pub enum InstKind {
    BinOp(BinOp, Operand, Operand),
    ICmp(IPred, Operand, Operand),
    FCmp(FPred, Operand, Operand),
    Call(FuncId, Vec<Operand>),
    /// `%p = alloca <ty>` — stack-allocate a slot of `ty`. Result type is Ptr.
    /// Used for mutable locals; mem2reg lifts these to SSA values at -O1+.
    Alloca(Type),
    /// `%v = load <ty>, <ptr>+<offset>` — load a value of `ty` from
    /// pointer + byte_offset. Offset is 0 for plain alloca-slot loads;
    /// non-zero for object field reads (offset = field_index * 8 in the
    /// MVP layout).
    Load(Type, Operand, u64),
    /// `store <value>, <ptr>+<offset>` — void result; value's type
    /// determines the store width. Same offset convention as Load.
    Store(Operand, Operand, u64),
    /// `%v = load_dyn <ty>, <ptr>+<dyn_byte_offset>` — like Load but the
    /// byte offset is an SSA value instead of a constant. Used for
    /// dynamic array indexing `xs[i]` where `i` isn't statically known.
    /// Backends compute `addr = base + offset` then load.
    LoadDyn(Type, Operand, Operand),
    /// `store_dyn <value>, <ptr>+<dyn_byte_offset>` — symmetric for the
    /// load. Used for `xs[i] = v`.
    StoreDyn(Operand, Operand, Operand),
    /// `%v = sitofp <i64-operand>` — signed integer to f64 cast. Used to
    /// promote i64 operands when mixed with f64 in arithmetic / comparisons.
    SiToFp(Operand),
    /// `%v = string_ref <id>` — yields a (ptr, len) pair to a global string
    /// constant. Result type is Ptr; the length lives in the module's
    /// `strings` table alongside the bytes.
    StringRef(StringId),
    /// `%v = fn_addr <fid>` — take the address of a known function.
    /// Result type is `Type::FnSig(sig_id)` matching the function's
    /// signature. M2 Phase B Stage 3.
    FnAddr(FuncId),
    /// `%v = call_indirect <sig_id>, <ptr>, <args>` — call through a
    /// function pointer. The signature is looked up via `module.signature(sig_id)`
    /// at codegen so the backend can build the right calling convention.
    /// M2 Phase B Stage 3.
    CallIndirect(SigId, Operand, Vec<Operand>),
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub result: Option<ValueId>, // None for void calls
    pub kind: InstKind,
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Br(BlockId),
    CondBr {
        cond: Operand,
        then_blk: BlockId,
        else_blk: BlockId,
    },
    Ret(Option<Operand>),
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Debug, Clone)]
pub struct ValueInfo {
    pub ty: Type,
    /// Debug-only display name. Pretty printer prefers this over the numeric
    /// id; codegen ignores it.
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<ValueId>,
    pub ret: Type,
    pub blocks: Vec<Block>,
    pub values: Vec<ValueInfo>, // index = ValueId.0
}

impl Function {
    pub fn new(name: impl Into<String>, ret: Type) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            ret,
            blocks: Vec::new(),
            values: Vec::new(),
        }
    }

    fn alloc_value(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        let id = ValueId(self.values.len() as u32);
        self.values.push(ValueInfo {
            ty,
            name: name.map(String::from),
        });
        id
    }

    pub fn add_param(&mut self, ty: Type, name: &str) -> ValueId {
        let id = self.alloc_value(ty, Some(name));
        self.params.push(id);
        id
    }

    pub fn add_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            id,
            insts: Vec::new(),
            term: Terminator::Unreachable,
        });
        id
    }

    pub fn append_inst(
        &mut self,
        block: BlockId,
        kind: InstKind,
        result_ty: Type,
        name: Option<&str>,
    ) -> ValueId {
        let result = self.alloc_value(result_ty, name);
        self.blocks[block.0 as usize].insts.push(Inst {
            result: Some(result),
            kind,
        });
        result
    }

    /// Append a void-result instruction (currently only `Call` to a void-returning function).
    pub fn append_void(&mut self, block: BlockId, kind: InstKind) {
        self.blocks[block.0 as usize].insts.push(Inst {
            result: None,
            kind,
        });
    }

    pub fn set_term(&mut self, block: BlockId, term: Terminator) {
        self.blocks[block.0 as usize].term = term;
    }

    pub fn value_type(&self, v: ValueId) -> Type {
        self.values[v.0 as usize].ty
    }

    pub fn value_name(&self, v: ValueId) -> String {
        match &self.values[v.0 as usize].name {
            Some(n) => format!("%{n}"),
            None => format!("%{}", v.0),
        }
    }

    /// True when the function is a forward declaration only — no blocks, no
    /// body. The codegen backend supplies the implementation (e.g. for
    /// runtime intrinsics like `print_i64`).
    pub fn is_declaration(&self) -> bool {
        self.blocks.is_empty()
    }
}

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

impl Function {
    fn write_to(&self, w: &mut String, m: &Module) -> std::fmt::Result {
        let kw = if self.is_declaration() { "extern fn" } else { "fn" };
        write!(w, "{kw} {}(", self.name)?;
        for (i, &p) in self.params.iter().enumerate() {
            if i > 0 {
                write!(w, ", ")?;
            }
            write!(w, "{}: {}", self.value_name(p), self.value_type(p).as_str())?;
        }
        write!(w, ") -> {}", self.ret.as_str())?;
        if self.is_declaration() {
            writeln!(w, ";")?;
            return Ok(());
        }
        writeln!(w, " {{")?;
        for block in &self.blocks {
            writeln!(w, "  bb{}:", block.id.0)?;
            for inst in &block.insts {
                self.write_inst(w, inst, m)?;
            }
            self.write_term(w, &block.term)?;
        }
        writeln!(w, "}}")?;
        Ok(())
    }

    fn write_operand(&self, w: &mut String, o: &Operand) -> std::fmt::Result {
        match o {
            Operand::Value(v) => write!(w, "{}", self.value_name(*v)),
            Operand::ConstI64(n) => write!(w, "{n}"),
            Operand::ConstI32(n) => write!(w, "{n}"),
            Operand::ConstF64(n) => write!(w, "{n}"),
            Operand::ConstBool(b) => write!(w, "{b}"),
            Operand::ConstPtrNull => write!(w, "null"),
        }
    }

    fn write_inst(&self, w: &mut String, inst: &Inst, m: &Module) -> std::fmt::Result {
        write!(w, "    ")?;
        if let Some(r) = inst.result {
            write!(w, "{} = ", self.value_name(r))?;
        }
        match &inst.kind {
            InstKind::BinOp(op, a, b) => {
                write!(w, "{} ", op.as_str())?;
                self.write_operand(w, a)?;
                write!(w, ", ")?;
                self.write_operand(w, b)?;
            }
            InstKind::ICmp(p, a, b) => {
                write!(w, "icmp {} ", p.as_str())?;
                self.write_operand(w, a)?;
                write!(w, ", ")?;
                self.write_operand(w, b)?;
            }
            InstKind::FCmp(p, a, b) => {
                write!(w, "fcmp {} ", p.as_str())?;
                self.write_operand(w, a)?;
                write!(w, ", ")?;
                self.write_operand(w, b)?;
            }
            InstKind::Call(fid, args) => {
                write!(w, "call {}(", m.func_name(*fid))?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(w, ", ")?;
                    }
                    self.write_operand(w, a)?;
                }
                write!(w, ")")?;
            }
            InstKind::Alloca(t) => {
                write!(w, "alloca {}", t.as_str())?;
            }
            InstKind::Load(t, ptr, offset) => {
                write!(w, "load {}, ", t.as_str())?;
                self.write_operand(w, ptr)?;
                if *offset != 0 {
                    write!(w, " +{offset}")?;
                }
            }
            InstKind::Store(val, ptr, offset) => {
                write!(w, "store ")?;
                self.write_operand(w, val)?;
                write!(w, ", ")?;
                self.write_operand(w, ptr)?;
                if *offset != 0 {
                    write!(w, " +{offset}")?;
                }
            }
            InstKind::LoadDyn(t, ptr, offset) => {
                write!(w, "load_dyn {}, ", t.as_str())?;
                self.write_operand(w, ptr)?;
                write!(w, " +")?;
                self.write_operand(w, offset)?;
            }
            InstKind::StoreDyn(val, ptr, offset) => {
                write!(w, "store_dyn ")?;
                self.write_operand(w, val)?;
                write!(w, ", ")?;
                self.write_operand(w, ptr)?;
                write!(w, " +")?;
                self.write_operand(w, offset)?;
            }
            InstKind::SiToFp(op) => {
                write!(w, "sitofp ")?;
                self.write_operand(w, op)?;
            }
            InstKind::StringRef(s) => {
                write!(w, "string_ref @str{}", s.0)?;
            }
            InstKind::FnAddr(fid) => {
                write!(w, "fn_addr {}", m.func_name(*fid))?;
            }
            InstKind::CallIndirect(sig, ptr, args) => {
                write!(w, "call_indirect <sig{}> ", sig.0)?;
                self.write_operand(w, ptr)?;
                write!(w, "(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(w, ", ")?;
                    }
                    self.write_operand(w, a)?;
                }
                write!(w, ")")?;
            }
        }
        writeln!(w)
    }

    fn write_term(&self, w: &mut String, t: &Terminator) -> std::fmt::Result {
        write!(w, "    ")?;
        match t {
            Terminator::Br(b) => writeln!(w, "br bb{}", b.0),
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                write!(w, "cond_br ")?;
                self.write_operand(w, cond)?;
                writeln!(w, ", bb{}, bb{}", then_blk.0, else_blk.0)
            }
            Terminator::Ret(Some(o)) => {
                write!(w, "ret ")?;
                self.write_operand(w, o)?;
                writeln!(w)
            }
            Terminator::Ret(None) => writeln!(w, "ret"),
            Terminator::Unreachable => writeln!(w, "unreachable"),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fib40_pretty_prints() {
        let m = demo_fib40();
        let mut s = String::new();
        m.write_to(&mut s).unwrap();
        // sanity: covers all the structural pieces the printer emits, not a
        // golden match — format is allowed to drift if the test still passes.
        assert!(s.contains("fn fib(%n: i64) -> i64"));
        assert!(s.contains("%t = icmp slt %n, 2"));
        assert!(s.contains("cond_br %t, bb1, bb2"));
        assert!(s.contains("ret %n"));
        assert!(s.contains("%a = sub %n, 1"));
        assert!(s.contains("%r1 = call fib(%a)"));
        assert!(s.contains("%s = add %r1, %r2"));
        assert!(s.contains("ret %s"));
    }
}
