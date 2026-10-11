//! Lowering of the typed number-box nodes to the bit operations of
//! the NaN-box encoding. The lowering emits `any_is_number` /
//! `any_is_int32` / `any_to_int32` / `any_to_f64` / `box_int32` /
//! `box_f64` so the number flow through an `any` value stays legible
//! to the egraph; once φ promotion has run and `anynum` has taken the
//! cells it can out of their boxes, every node left is expanded here.
//!
//! The encoding (torajs-anyvalue `nanbox`): an int32 sits in the low
//! 32 bits under `TAG_TYPE_NUMBER` in the top 16; a double is its
//! bits plus `DOUBLE_ENCODE_OFFSET`, which lands every non-NaN in
//! `[OFFSET, TAG)`. Cells, ShortStr and the sentinels have the top 16
//! bits of `TAG_TYPE_NUMBER` clear, so one `and` tells a number apart.
//!
//! ```text
//! any_is_number v     and bits, TAG ; icmp ne 0
//! any_is_int32 v      lshr bits, 48 ; icmp eq TAG >> 48
//! any_to_int32 v      shl bits, 32 ; ashr 32
//! any_to_f64 v, b     b ? sitofp (any_to_int32 v) : bitcast (bits - OFFSET)
//! box_int32 i         and i, 0xFFFF_FFFF ; or TAG ; inttoptr
//! box_f64 f           f != f ? canonical NaN : inttoptr (bitcast f + OFFSET)
//! ```
//!
//! `bits` is `ptrtoint v`, a register move at codegen; the nodes over
//! one single-def value share the one a dominating block made, as
//! the test and the decode of an addend do.
//!
//! The two branching forms split their block and define the node's
//! value by a `copy` on each arm (the multi-def form φ promotion
//! leaves behind). Only a NaN can encode into another tag's range —
//! its payload wraps past the double window — so every NaN stores the
//! canonical one and one `fcmp` replaces a two-mask range test.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{
    BinOp, Block, BlockId, DOUBLE_ENCODE_OFFSET, FPred, Function, IPred, Inst, InstKind, Module,
    Operand, TAG_TYPE_NUMBER, Terminator, Type, ValueId,
};

use crate::float_demote::cfg::multi_def_values;
use crate::float_demote::push_value;
use crate::float_demote::split_block_at;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExpandStats {
    pub nodes_expanded: u32,
}

const TAG: i64 = TAG_TYPE_NUMBER as i64;
const OFFSET: i64 = DOUBLE_ENCODE_OFFSET as i64;
const INT_TAG_TOP: i64 = (TAG_TYPE_NUMBER >> 48) as i64;

/// The canonical NaN's encoding.
pub(crate) fn canonical_nan_bits() -> i64 {
    f64::NAN.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET) as i64
}

/// The encoding of a constant double.
pub(crate) fn box_f64_const(c: f64) -> i64 {
    if c.is_nan() {
        canonical_nan_bits()
    } else {
        c.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET) as i64
    }
}

/// The encoding of a constant that fits an int32.
pub(crate) fn box_int32_const(i: i64) -> i64 {
    (i & 0xFFFF_FFFF) | TAG
}

pub fn expand_number_nodes(module: &mut Module) -> ExpandStats {
    let mut stats = ExpandStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let dense = func
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.id.0 as usize == i);
        let any = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .any(|i| is_number_node(&i.kind));
        if dense && any {
            Expander::new(func).run(func, &mut stats);
        }
    }
    stats
}

pub(crate) fn is_number_node(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::AnyIsNumber(_)
            | InstKind::AnyIsInt32(_)
            | InstKind::AnyToInt32(_)
            | InstKind::AnyToF64(_, _)
            | InstKind::BoxInt32(_)
            | InstKind::BoxF64(_)
    )
}

fn mk(result: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(result),
        kind,
        origin: None,
    }
}

fn val(v: ValueId) -> Operand {
    Operand::Value(v)
}

/// The expansion over one function, sharing `ptrtoint` across nodes.
struct Expander {
    dom: DominatorTree,
    /// the block each block was carved from (itself for an original)
    origin: Vec<BlockId>,
    cells: HashSet<ValueId>,
    /// single-def value → its bits and the origin of the block that
    /// made them, on the main line (never inside an arm)
    bits: HashMap<ValueId, (ValueId, BlockId)>,
}

impl Expander {
    fn new(func: &Function) -> Self {
        Expander {
            dom: DominatorTree::compute(func),
            origin: func.blocks.iter().map(|b| b.id).collect(),
            cells: multi_def_values(func),
            bits: HashMap::default(),
        }
    }

    fn run(&mut self, func: &mut Function, stats: &mut ExpandStats) {
        let mut b = 0;
        while b < func.blocks.len() {
            let mut i = 0;
            while i < func.blocks[b].insts.len() {
                if !is_number_node(&func.blocks[b].insts[i].kind) {
                    i += 1;
                    continue;
                }
                stats.nodes_expanded += 1;
                let inst = func.blocks[b].insts[i].clone();
                let result = inst.result.expect("a number node defines a value");
                let block = BlockId(b as u32);
                match inst.kind {
                    // the rest of the block moved to the continuation,
                    // which the outer loop reaches in its turn
                    InstKind::AnyToF64(v, flag) => {
                        self.expand_to_f64(func, block, i, result, v, flag);
                        break;
                    }
                    InstKind::BoxF64(Operand::ConstF64(c)) => {
                        func.blocks[b].insts[i] = mk(
                            result,
                            InstKind::IntToPtr(Operand::ConstI64(box_f64_const(c))),
                        );
                        i += 1;
                    }
                    InstKind::BoxF64(f) => {
                        self.expand_box_f64(func, block, i, result, f);
                        break;
                    }
                    kind => {
                        let seq = self.straight(func, block, result, &kind);
                        let n = seq.len();
                        func.blocks[b].insts.splice(i..=i, seq);
                        i += n;
                    }
                }
            }
            b += 1;
        }
    }

    /// `ptrtoint v` as an I64: the one a dominating main-line block
    /// already made for a single-def `v`, else a fresh one, remembered
    /// when made on the main line (`share`).
    fn bits_of(
        &mut self,
        func: &mut Function,
        v: Operand,
        at: BlockId,
        share: bool,
        out: &mut Vec<Inst>,
    ) -> ValueId {
        let single = match v {
            Operand::Value(x) if !self.cells.contains(&x) => Some(x),
            _ => None,
        };
        if let Some(x) = single
            && let Some((bits, from)) = self.bits.get(&x)
            && self.dom.dominates(*from, self.origin[at.0 as usize])
        {
            return *bits;
        }
        let bits = push_value(&mut func.values, Type::I64);
        out.push(mk(bits, InstKind::PtrToInt(v)));
        if share && let Some(x) = single {
            self.bits.insert(x, (bits, self.origin[at.0 as usize]));
        }
        bits
    }

    /// The straight-line expansions, `result` defined last.
    fn straight(
        &mut self,
        func: &mut Function,
        at: BlockId,
        result: ValueId,
        kind: &InstKind,
    ) -> Vec<Inst> {
        let mut out = Vec::with_capacity(3);
        match kind {
            InstKind::AnyIsNumber(v) => {
                let bits = self.bits_of(func, *v, at, true, &mut out);
                let t = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    t,
                    InstKind::BinOp(BinOp::And, val(bits), Operand::ConstI64(TAG)),
                ));
                out.push(mk(
                    result,
                    InstKind::ICmp(IPred::Ne, val(t), Operand::ConstI64(0)),
                ));
            }
            InstKind::AnyIsInt32(v) => {
                let bits = self.bits_of(func, *v, at, true, &mut out);
                let top = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    top,
                    InstKind::BinOp(BinOp::LShr, val(bits), Operand::ConstI64(48)),
                ));
                out.push(mk(
                    result,
                    InstKind::ICmp(IPred::Eq, val(top), Operand::ConstI64(INT_TAG_TOP)),
                ));
            }
            InstKind::AnyToInt32(v) => {
                let bits = self.bits_of(func, *v, at, true, &mut out);
                sext32(func, bits, result, &mut out);
            }
            InstKind::BoxInt32(Operand::ConstI64(i)) => {
                out.push(mk(
                    result,
                    InstKind::IntToPtr(Operand::ConstI64(box_int32_const(*i))),
                ));
            }
            InstKind::BoxInt32(i) => {
                let low = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    low,
                    InstKind::BinOp(BinOp::And, *i, Operand::ConstI64(0xFFFF_FFFF)),
                ));
                let boxed = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    boxed,
                    InstKind::BinOp(BinOp::Or, val(low), Operand::ConstI64(TAG)),
                ));
                out.push(mk(result, InstKind::IntToPtr(val(boxed))));
            }
            other => unreachable!("not a straight-line number node: {other:?}"),
        }
        out
    }

    /// Cut `block` before `at`, drop the node at `at`, and return the
    /// continuation the arms branch to.
    fn cut(&mut self, func: &mut Function, block: BlockId, at: usize) -> BlockId {
        let cont = split_block_at(func, block, &[at])[0];
        func.blocks[cont.0 as usize].insts.remove(0);
        self.origin.push(self.origin[block.0 as usize]);
        cont
    }

    fn arm(
        &mut self,
        func: &mut Function,
        from: BlockId,
        insts: Vec<Inst>,
        cont: BlockId,
    ) -> BlockId {
        let id = BlockId(func.blocks.len() as u32);
        func.blocks.push(Block {
            id,
            insts,
            term: Terminator::Br(cont),
        });
        self.origin.push(self.origin[from.0 as usize]);
        id
    }

    /// `any_to_f64 v, flag`: the int32 arm sign-extends and converts,
    /// the double arm subtracts the offset and reinterprets.
    fn expand_to_f64(
        &mut self,
        func: &mut Function,
        block: BlockId,
        at: usize,
        result: ValueId,
        v: Operand,
        flag: Operand,
    ) {
        let cont = self.cut(func, block, at);
        let mut int_insts = Vec::with_capacity(5);
        let bits = self.bits_of(func, v, block, false, &mut int_insts);
        let n = push_value(&mut func.values, Type::I64);
        sext32(func, bits, n, &mut int_insts);
        let f = push_value(&mut func.values, Type::F64);
        int_insts.push(mk(f, InstKind::SiToFp(val(n))));
        int_insts.push(mk(result, InstKind::Copy(Type::F64, val(f))));
        let int_blk = self.arm(func, block, int_insts, cont);

        let mut dbl_insts = Vec::with_capacity(4);
        let bits = self.bits_of(func, v, block, false, &mut dbl_insts);
        let raw = push_value(&mut func.values, Type::I64);
        dbl_insts.push(mk(
            raw,
            InstKind::BinOp(BinOp::Sub, val(bits), Operand::ConstI64(OFFSET)),
        ));
        let f = push_value(&mut func.values, Type::F64);
        dbl_insts.push(mk(f, InstKind::BitCastI64ToF64(val(raw))));
        dbl_insts.push(mk(result, InstKind::Copy(Type::F64, val(f))));
        let dbl_blk = self.arm(func, block, dbl_insts, cont);
        cond_br(func, block, flag, int_blk, dbl_blk);
    }

    /// `box_f64 f`: a NaN stores the canonical encoding, anything else
    /// its bits plus the offset.
    fn expand_box_f64(
        &mut self,
        func: &mut Function,
        block: BlockId,
        at: usize,
        result: ValueId,
        f: Operand,
    ) {
        let is_nan = push_value(&mut func.values, Type::Bool);
        func.blocks[block.0 as usize]
            .insts
            .insert(at, mk(is_nan, InstKind::FCmp(FPred::Une, f, f)));
        let cont = self.cut(func, block, at + 1);

        let mut ok_insts = Vec::with_capacity(4);
        let raw = push_value(&mut func.values, Type::I64);
        ok_insts.push(mk(raw, InstKind::BitCastF64ToI64(f)));
        let enc = push_value(&mut func.values, Type::I64);
        ok_insts.push(mk(
            enc,
            InstKind::BinOp(BinOp::Add, val(raw), Operand::ConstI64(OFFSET)),
        ));
        let v = push_value(&mut func.values, Type::Any);
        ok_insts.push(mk(v, InstKind::IntToPtr(val(enc))));
        ok_insts.push(mk(result, InstKind::Copy(Type::Any, val(v))));
        let ok_blk = self.arm(func, block, ok_insts, cont);

        let v = push_value(&mut func.values, Type::Any);
        let nan_insts = vec![
            mk(
                v,
                InstKind::IntToPtr(Operand::ConstI64(canonical_nan_bits())),
            ),
            mk(result, InstKind::Copy(Type::Any, val(v))),
        ];
        let nan_blk = self.arm(func, block, nan_insts, cont);
        cond_br(func, block, val(is_nan), nan_blk, ok_blk);
    }
}

/// The low 32 bits of `bits`, sign-extended into `result`.
fn sext32(func: &mut Function, bits: ValueId, result: ValueId, out: &mut Vec<Inst>) {
    let hi = push_value(&mut func.values, Type::I64);
    out.push(mk(
        hi,
        InstKind::BinOp(BinOp::Shl, val(bits), Operand::ConstI64(32)),
    ));
    out.push(mk(
        result,
        InstKind::BinOp(BinOp::AShr, val(hi), Operand::ConstI64(32)),
    ));
}

fn cond_br(
    func: &mut Function,
    block: BlockId,
    cond: Operand,
    then_blk: BlockId,
    else_blk: BlockId,
) {
    func.blocks[block.0 as usize].term = Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueInfo;

    fn func_with(kind: InstKind, ty: Type) -> Function {
        let values = [Type::Any, Type::Bool, ty]
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect();
        Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: ty,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![mk(ValueId(2), kind)],
                term: Terminator::Ret(Some(val(ValueId(2)))),
            }],
            values,
            current_origin: None,
        }
    }

    fn all_kinds(f: &Function) -> Vec<&InstKind> {
        f.blocks
            .iter()
            .flat_map(|b| b.insts.iter().map(|i| &i.kind))
            .collect()
    }

    #[test]
    fn straight_nodes_expand_in_place() {
        let mut m = Module::default();
        m.funcs
            .push(func_with(InstKind::AnyIsInt32(val(ValueId(0))), Type::Bool));
        m.funcs.push(func_with(
            InstKind::BoxInt32(Operand::ConstI64(-5)),
            Type::Any,
        ));
        let st = expand_number_nodes(&mut m);
        assert_eq!(st.nodes_expanded, 2);
        let f = &m.funcs[0];
        assert_eq!(f.blocks.len(), 1);
        assert!(matches!(
            all_kinds(f).as_slice(),
            [
                InstKind::PtrToInt(_),
                InstKind::BinOp(BinOp::LShr, _, Operand::ConstI64(48)),
                InstKind::ICmp(IPred::Eq, _, Operand::ConstI64(INT_TAG_TOP)),
            ]
        ));
        let g = &m.funcs[1];
        let enc = (-5i64 & 0xFFFF_FFFF) | TAG;
        assert!(matches!(
            all_kinds(g).as_slice(),
            [InstKind::IntToPtr(Operand::ConstI64(e))] if *e == enc
        ));
        assert!(all_kinds(g).iter().all(|k| !is_number_node(k)));
    }

    #[test]
    fn branching_nodes_split_the_block() {
        let mut m = Module::default();
        m.funcs.push(func_with(
            InstKind::AnyToF64(val(ValueId(0)), val(ValueId(1))),
            Type::F64,
        ));
        let mut g = func_with(InstKind::BoxF64(val(ValueId(3))), Type::Any);
        g.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
        g.params.push(ValueId(3));
        m.funcs.push(g);
        expand_number_nodes(&mut m);
        for f in &m.funcs {
            // entry, continuation, two arms
            assert_eq!(f.blocks.len(), 4);
            assert!(matches!(f.blocks[0].term, Terminator::CondBr { .. }));
            assert!(matches!(
                f.blocks[1].term,
                Terminator::Ret(Some(Operand::Value(ValueId(2))))
            ));
            let defs = f
                .blocks
                .iter()
                .flat_map(|b| b.insts.iter())
                .filter(|i| i.result == Some(ValueId(2)))
                .count();
            assert_eq!(defs, 2, "one copy per arm");
            assert!(all_kinds(f).iter().all(|k| !is_number_node(k)));
        }
        let nan: Vec<i64> = all_kinds(&m.funcs[1])
            .iter()
            .filter_map(|k| match k {
                InstKind::IntToPtr(Operand::ConstI64(c)) => Some(*c),
                _ => None,
            })
            .collect();
        assert_eq!(nan, vec![canonical_nan_bits()]);
    }

    /// The test and the decode of one value share its bits: the
    /// `any_is_number` in the entry makes them, the `any_is_int32`
    /// and both arms of the `any_to_f64` in the block it dominates
    /// read them.
    #[test]
    fn nodes_over_one_value_share_its_bits() {
        let values = [Type::Any, Type::Bool, Type::Bool, Type::F64]
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect();
        let f = Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![mk(ValueId(1), InstKind::AnyIsNumber(val(ValueId(0))))],
                    term: Terminator::CondBr {
                        cond: val(ValueId(1)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![
                        mk(ValueId(2), InstKind::AnyIsInt32(val(ValueId(0)))),
                        mk(
                            ValueId(3),
                            InstKind::AnyToF64(val(ValueId(0)), val(ValueId(2))),
                        ),
                    ],
                    term: Terminator::Ret(Some(val(ValueId(3)))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstF64(0.0))),
                },
            ],
            values,
            current_origin: None,
        };
        let mut m = Module::default();
        m.funcs.push(f);
        expand_number_nodes(&mut m);
        let casts = all_kinds(&m.funcs[0])
            .iter()
            .filter(|k| matches!(k, InstKind::PtrToInt(_)))
            .count();
        assert_eq!(casts, 1);
    }
}
