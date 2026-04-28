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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I64,
    F64,
    I32,
    Bool,
    Void,
}

impl Type {
    pub fn as_str(self) -> &'static str {
        match self {
            Type::I64 => "i64",
            Type::F64 => "f64",
            Type::I32 => "i32",
            Type::Bool => "bool",
            Type::Void => "void",
        }
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
