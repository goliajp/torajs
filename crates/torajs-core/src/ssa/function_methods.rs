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

mod mnemonic;

use mnemonic::{one_operand_mnemonic, pin_word};

use super::{
    BlockId, Function, Inst, InstKind, Module, Operand, SuspendLane, Terminator, Type, ValueId,
    ValueInfo,
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

    /// Contract check shared by the append paths: a block whose
    /// terminator has been set (no longer the `Unreachable` open
    /// placeholder) must not receive further instructions — they
    /// would sit before the terminator and execute, which is how the
    /// dead-sibling-after-throw class of bugs ships. Lowering walks
    /// guard with `cur_open()`; this backstop catches any site that
    /// forgets.
    ///
    /// The entry block (bb0) is exempt: it is the prologue slot —
    /// `alloca_in_entry` slot reservations and their zero-init
    /// stores are deliberately appended there for the whole lowering
    /// run, long after its terminator is set. They sit physically
    /// before the terminator and run once on entry, which is exactly
    /// the prologue semantics wanted (the textbook position that
    /// dominates every use).
    #[inline]
    fn debug_assert_open(&self, block: BlockId, _kind: &InstKind) {
        debug_assert!(
            block.0 == 0 || matches!(self.blocks[block.0 as usize].term, Terminator::Unreachable),
            "append into terminated block bb{} of `{}` — instructions \
             added after set_term execute before the terminator",
            block.0,
            self.name
        );
    }

    pub fn append_inst(
        &mut self,
        block: BlockId,
        kind: InstKind,
        result_ty: Type,
        name: Option<&str>,
    ) -> ValueId {
        self.debug_assert_open(block, &kind);
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
        self.debug_assert_open(block, &kind);
        let origin = self.current_origin;
        self.blocks[block.0 as usize].insts.push(Inst {
            result: None,
            kind,
            origin,
        });
    }

    /// Insert an instruction at `idx` of `block`, ahead of the one there.
    pub fn insert_inst(
        &mut self,
        block: BlockId,
        idx: usize,
        kind: InstKind,
        result_ty: Type,
    ) -> ValueId {
        let result = self.alloc_value(result_ty, None);
        let origin = self.current_origin;
        self.blocks[block.0 as usize].insts.insert(
            idx,
            Inst {
                result: Some(result),
                kind,
                origin,
            },
        );
        result
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

    /// `(a, b, …)`
    fn write_operand_list(&self, w: &mut String, ops: &[Operand]) -> std::fmt::Result {
        write!(w, "(")?;
        for (i, o) in ops.iter().enumerate() {
            if i > 0 {
                write!(w, ", ")?;
            }
            self.write_operand(w, o)?;
        }
        write!(w, ")")
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
            InstKind::LoadDynScaled8(t, ptr, idx) => {
                write!(w, "load_dyn_s8 {}, ", t.as_str())?;
                self.write_operand(w, ptr)?;
                write!(w, " + 8*")?;
                self.write_operand(w, idx)?;
            }
            InstKind::LoadU8Dyn(ptr, idx) => {
                write!(w, "load_u8_dyn ")?;
                self.write_operand(w, ptr)?;
                write!(w, " +")?;
                self.write_operand(w, idx)?;
            }
            InstKind::LoadU16Dyn(ptr, idx) => {
                write!(w, "load_u16_dyn ")?;
                self.write_operand(w, ptr)?;
                write!(w, " + 2*")?;
                self.write_operand(w, idx)?;
            }
            InstKind::StoreDynScaled8(val, ptr, idx) => {
                write!(w, "store_dyn_s8 ")?;
                self.write_operand(w, val)?;
                write!(w, ", ")?;
                self.write_operand(w, ptr)?;
                write!(w, " + 8*")?;
                self.write_operand(w, idx)?;
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
            InstKind::BoxedEntryAddr(fid) => {
                write!(w, "boxed_entry_addr {}", m.func_name(*fid))?;
            }
            InstKind::ThrowSignal => {
                write!(w, "throw_signal")?;
            }
            InstKind::Pinned(word) => write!(w, "pinned {}", pin_word(*word))?,
            InstKind::PinSet(word, v) => {
                write!(w, "pin_set {}, ", pin_word(*word))?;
                self.write_operand(w, v)?;
            }
            InstKind::Suspend(kind, lane, src, held) => {
                match lane {
                    SuspendLane::Direct => write!(w, "suspend {} direct ", kind.name())?,
                    SuspendLane::Promise(t) => {
                        write!(w, "suspend {} promise<{}> ", kind.name(), t.as_str())?
                    }
                }
                self.write_operand(w, src)?;
                if !held.is_empty() {
                    write!(w, " held")?;
                    self.write_operand_list(w, held)?;
                }
            }
            InstKind::CallIndirect(sig, ptr, args) => {
                write!(w, "call_indirect <sig{}> ", sig.0)?;
                self.write_operand(w, ptr)?;
                self.write_operand_list(w, args)?;
            }
            InstKind::Copy(ty, op) => {
                write!(w, "copy {} ", ty.as_str())?;
                self.write_operand(w, op)?;
            }
            InstKind::Select(ty, cond, t, e) => {
                write!(w, "select {} ", ty.as_str())?;
                self.write_operand(w, cond)?;
                write!(w, ", ")?;
                self.write_operand(w, t)?;
                write!(w, ", ")?;
                self.write_operand(w, e)?;
            }
            InstKind::Vec(vi) => {
                write!(w, "{}", vi.mnemonic())?;
                for (i, op) in vi.operands().into_iter().flatten().enumerate() {
                    write!(w, "{}", if i == 0 { " " } else { ", " })?;
                    self.write_operand(w, op)?;
                }
            }
            InstKind::CtpopRangeSum(start, bound, acc) => {
                write!(w, "ctpop.range.sum ")?;
                self.write_operand(w, start)?;
                write!(w, ", ")?;
                self.write_operand(w, bound)?;
                write!(w, ", ")?;
                self.write_operand(w, acc)?;
            }
            InstKind::AnyToF64(v, flag) => {
                write!(w, "any_to_f64 ")?;
                self.write_operand(w, v)?;
                write!(w, ", ")?;
                self.write_operand(w, flag)?;
            }
            InstKind::RcInc(v)
            | InstKind::RcIncAny(v)
            | InstKind::RcDecAny(v)
            | InstKind::BoxAny(_, v)
            | InstKind::AnyIsNumber(v)
            | InstKind::AnyIsInt32(v)
            | InstKind::AnyToInt32(v)
            | InstKind::BoxInt32(v)
            | InstKind::BoxF64(v)
            | InstKind::SiToFp(v)
            | InstKind::FpToSi(v)
            | InstKind::FpToInt32(v)
            | InstKind::ZExtBoolToI64(v)
            | InstKind::ZExtI32ToI64(v)
            | InstKind::BitCastF64ToI64(v)
            | InstKind::BitCastI64ToF64(v)
            | InstKind::IntToPtr(v)
            | InstKind::PtrToInt(v)
            | InstKind::TruncI64ToBool(v)
            | InstKind::Identity(v)
            | InstKind::Neg(v)
            | InstKind::Ctpop(v)
            | InstKind::Cttz(v) => {
                write!(w, "{} ", one_operand_mnemonic(&inst.kind))?;
                self.write_operand(w, v)?;
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
