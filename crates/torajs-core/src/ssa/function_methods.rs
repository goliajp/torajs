//! Constructor + builder + pretty-printer impls for `Function`.
//!
//! Two `impl Function` blocks (Rust allows multiple impl blocks for
//! the same type in the same crate):
//!
//! - **Constructor / builder** (first block): `new`, `alloc_value`,
//!   `add_param`, `add_block`, `append_inst`, `append_void`,
//!   `set_term`, `value_type`, `value_name`, `is_declaration`.
//! - **Pretty printer** (second block): `write_to`, `write_operand`,
//!   `write_inst`, `write_term` — LLVM-IR-shaped output used by
//!   `tr ssa` dump and the demo helpers.
//!
//! Extracted from `ssa.rs` (2026-05-25, god-file decomp batch 16).

use std::fmt::Write;

use super::{
    BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

impl Function {
    pub fn new(name: impl Into<String>, ret: Type) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            ret,
            blocks: Vec::new(),
            values: Vec::new(),
            current_origin: None,
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
        self.blocks.push(super::Block {
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
        let origin = self.current_origin;
        self.blocks[block.0 as usize].insts.push(Inst {
            result: Some(result),
            kind,
            origin,
        });
        result
    }

    /// Append a void-result instruction (currently only `Call` to a void-returning function).
    pub fn append_void(&mut self, block: BlockId, kind: InstKind) {
        let origin = self.current_origin;
        self.blocks[block.0 as usize].insts.push(Inst {
            result: None,
            kind,
            origin,
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

impl Function {
    pub(super) fn write_to(&self, w: &mut String, m: &Module) -> std::fmt::Result {
        let kw = if self.is_declaration() {
            "extern fn"
        } else {
            "fn"
        };
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
            InstKind::AllocaBytes(n) => {
                write!(w, "alloca_bytes {n}")?;
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
            InstKind::FpToSi(op) => {
                write!(w, "fptosi ")?;
                self.write_operand(w, op)?;
            }
            InstKind::ZExtBoolToI64(op) => {
                write!(w, "zext_bool ")?;
                self.write_operand(w, op)?;
            }
            InstKind::BitCastF64ToI64(op) => {
                write!(w, "bitcast_f64_to_i64 ")?;
                self.write_operand(w, op)?;
            }
            InstKind::BitCastI64ToF64(op) => {
                write!(w, "bitcast_i64_to_f64 ")?;
                self.write_operand(w, op)?;
            }
            InstKind::IntToPtr(op) => {
                write!(w, "inttoptr ")?;
                self.write_operand(w, op)?;
            }
            InstKind::TruncI64ToBool(op) => {
                write!(w, "trunc_i64_to_bool ")?;
                self.write_operand(w, op)?;
            }
            InstKind::StringRef(s) => {
                write!(w, "string_ref @str{}", s.0)?;
            }
            InstKind::StaticStrRef(s) => {
                write!(w, "static_str_ref @str{}", s.0)?;
            }
            InstKind::GlobalRef(name) => {
                write!(w, "global_ref @{name}")?;
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
