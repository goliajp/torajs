use super::super::{LOOP_VECTORS_MAX, pair_module};
use super::*;
use torajs_core::ssa::{Block, IPred, Module, ValueInfo};

const N0: ValueId = ValueId(0);
const N1: ValueId = ValueId(1);

/// A function under construction: its values and their types.
struct Build {
    types: Vec<Type>,
}

impl Build {
    fn val(&mut self, ty: Type) -> ValueId {
        self.types.push(ty);
        ValueId(self.types.len() as u32 - 1)
    }

    fn def(&mut self, out: &mut Vec<Inst>, ty: Type, kind: InstKind) -> ValueId {
        let r = self.val(ty);
        out.push(inst(r, kind));
        r
    }
}

/// `k` counters `d_j = 1, 2, …` stepped together, each dividing the
/// dividends `%0`, `%1` (i64 parameters) in the one loop block:
///
/// ```text
/// bb0: a0 = sitofp %0; a1 = sitofp %1; d_j = copy 1; (sums = copy 0); br bb1
/// bb1: c = icmp sle d_0, 100; cond_br c, bb2, bb3
/// bb2: per j: y = sitofp d_j; q = fdiv a0, y; t = fptosi q; ...
///             (q' = fdiv a1, y; t' = fptosi q' likewise)
///      per j: w = add d_j, 1; d_j = copy w; br bb1
/// bb3: ret
/// ```
///
/// With `sums`, each quotient tests divisibility and adds `d_j` to its
/// own sum (`if (n % d === 0) s += d`), which accum takes onto the
/// lanes; else the two conversions are added and dropped.
fn counters(k: usize, sums: bool) -> Function {
    use Type::{Bool, F64, I64};
    let mut b = Build {
        types: vec![I64, I64],
    };
    let v = Operand::Value;
    let one = Operand::ConstI64(1);
    let mut pre = Vec::new();
    let a0 = b.def(&mut pre, F64, InstKind::SiToFp(v(N0)));
    let a1 = b.def(&mut pre, F64, InstKind::SiToFp(v(N1)));
    let ds: Vec<ValueId> = (0..k)
        .map(|_| b.def(&mut pre, I64, InstKind::Copy(I64, one)))
        .collect();
    let zero = || InstKind::Copy(I64, Operand::ConstI64(0));
    let ss: Vec<[ValueId; 2]> = (0..if sums { k } else { 0 })
        .map(|_| [(); 2].map(|_| b.def(&mut pre, I64, zero())))
        .collect();
    let mut head = Vec::new();
    let c = b.def(
        &mut head,
        Bool,
        InstKind::ICmp(IPred::Sle, v(ds[0]), Operand::ConstI64(100)),
    );
    let mut body = Vec::new();
    let mut carries = Vec::new();
    for (j, &d) in ds.iter().enumerate() {
        let y = b.def(&mut body, F64, InstKind::SiToFp(v(d)));
        let mut ts = Vec::new();
        for a in [a0, a1] {
            let q = b.def(&mut body, F64, InstKind::BinOp(BinOp::FDiv, v(a), v(y)));
            let t = b.def(&mut body, I64, InstKind::FpToSi(v(q)));
            ts.push(t);
        }
        if !sums {
            b.def(
                &mut body,
                I64,
                InstKind::BinOp(BinOp::Add, v(ts[0]), v(ts[1])),
            );
            continue;
        }
        let dd = b.def(&mut body, I64, InstKind::Copy(I64, v(d)));
        for ((t, n), s) in ts.iter().zip([N0, N1]).zip(ss[j]) {
            let p = b.def(&mut body, I64, InstKind::BinOp(BinOp::Mul, v(*t), v(d)));
            let u = b.def(&mut body, I64, InstKind::BinOp(BinOp::Add, v(s), v(dd)));
            let eq = b.def(&mut body, Bool, InstKind::ICmp(IPred::Eq, v(p), v(n)));
            let w = b.def(&mut body, I64, InstKind::Select(I64, v(eq), v(u), v(s)));
            carries.push(inst(s, InstKind::Copy(I64, v(w))));
        }
    }
    body.append(&mut carries);
    for &d in &ds {
        let w = b.def(&mut body, I64, InstKind::BinOp(BinOp::Add, v(d), one));
        body.push(inst(d, InstKind::Copy(I64, v(w))));
    }
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    Function {
        name: "f".into(),
        params: vec![N0, N1],
        ret: I64,
        blocks: vec![
            block(0, pre, Terminator::Br(BlockId(1))),
            block(
                1,
                head,
                Terminator::CondBr {
                    cond: v(c),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ],
        values: b
            .types
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect(),
        current_origin: None,
    }
}

/// The dividends in `[0, 2^31]`, every counter in `[1, 101]`: the
/// function and the pass's (pairs, accumulated, f64 cells, i64 cells).
fn run(f: Function) -> (Function, [u32; 4]) {
    let counters = f.blocks[0]
        .insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Copy(Type::I64, Operand::ConstI64(1))))
        .filter_map(|i| i.result);
    let mut known: HashMap<ValueId, NumFact> = [N0, N1]
        .into_iter()
        .map(|n| (n, NumFact::new(0, 1 << 31)))
        .collect();
    known.extend(counters.map(|d| (d, NumFact::new(1, 101))));
    let mut m = Module::default();
    m.funcs.push(f);
    let s = pair_module(&mut m, Some(&[known]));
    let f = m.funcs.pop().expect("one function");
    let counts = [s.pairs, s.accumulated, s.float_counters, s.int_counters];
    (f, counts)
}

/// The vectors the loop holds, as `LOOP_VECTORS_MAX` counts them: the
/// preheader's vectors the loop reads, and one per splat in the loop.
fn held(f: &Function) -> usize {
    let in_loop: Vec<&Inst> = f.blocks[1..f.blocks.len() - 1]
        .iter()
        .flat_map(|b| b.insts.iter())
        .collect();
    let read = |x: ValueId| {
        in_loop.iter().any(|i| {
            let mut hit = false;
            visit_value_operands(&i.kind, |o| hit |= o == x);
            hit
        })
    };
    let pre = f.blocks[0]
        .insts
        .iter()
        .filter_map(|i| i.result)
        .filter(|r| matches!(f.values[r.0 as usize].ty, Type::Vec(_)) && read(*r))
        .count();
    let splats = in_loop
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::Splat(..))))
        .count();
    pre + splats
}

#[test]
fn one_accumulating_counter_takes_both_cells() {
    let (f, counts) = run(counters(1, true));
    assert_eq!(counts, [1, 1, 1, 1]);
    assert!(held(&f) <= LOOP_VECTORS_MAX);
}

#[test]
fn accumulating_counters_take_cells_while_they_fit() {
    // two pairs, two sums and two splats of each kind hold 8; the four
    // cells take one more each
    let (f, counts) = run(counters(2, true));
    assert_eq!(counts, [2, 2, 2, 2]);
    assert_eq!(held(&f), LOOP_VECTORS_MAX);
    // four hold 16 splatted already: no cell, every divisor splatted
    let (f, counts) = run(counters(4, true));
    assert_eq!(counts, [4, 4, 0, 0]);
    assert_eq!(held(&f), 16);
    let splats = f.blocks[2]
        .insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::Splat(..))))
        .count();
    assert_eq!(splats, 8);
}

#[test]
fn four_counters_on_plain_pairs_all_fit() {
    let (f, counts) = run(counters(4, false));
    assert_eq!(counts, [4, 0, 4, 0]);
    assert_eq!(held(&f), LOOP_VECTORS_MAX);
}

/// The one counter's f64 cell, the step's add and its copy in the loop
/// block, at their positions.
fn step_at(body: &[Inst]) -> (ValueId, usize) {
    let add = body
        .iter()
        .position(|i| matches!(i.kind, InstKind::Vec(VecInst::BinOp(VecOp::FAdd, ..))))
        .expect("a stepped cell");
    let InstKind::Vec(VecInst::BinOp(_, Operand::Value(cell), _)) = body[add].kind else {
        unreachable!();
    };
    assert_eq!(body[add + 1].result, Some(cell));
    (cell, add)
}

#[test]
fn a_write_ahead_of_the_reads_steps_the_cell_there() {
    let mut f = counters(1, false);
    // `w = add d, 1; d = copy w` first, so every read sees the new `d`
    let body = &mut f.blocks[2].insts;
    let step: Vec<Inst> = body.drain(body.len() - 2..).collect();
    body.splice(0..0, step);
    let d = body[1].result;
    let (f, counts) = run(f);
    assert_eq!(counts, [1, 0, 1, 0]);
    let body = &f.blocks[2].insts;
    let (cell, add) = step_at(body);
    // the step follows the write and comes before the divide
    assert_eq!(add, 2);
    assert_eq!(body[1].result, d);
    let div = body
        .iter()
        .position(|i| matches!(i.kind, InstKind::Vec(VecInst::BinOp(VecOp::FDiv, ..))));
    assert!(div.is_some_and(|at| at > add + 1));
    assert!(matches!(
        body[div.unwrap_or(0)].kind,
        InstKind::Vec(VecInst::BinOp(_, _, Operand::Value(y))) if y == cell
    ));
}

#[test]
fn a_write_some_trips_skip_steps_the_cell_with_it() {
    let mut f = counters(1, false);
    // `if (c) d++`: the write in a block of its own, skipped when the
    // first quotient is odd
    let mut body = std::mem::take(&mut f.blocks[2].insts);
    let write: Vec<Inst> = body.drain(body.len() - 2..).collect();
    let t = body[2].result.expect("the first conversion");
    let odd = ValueId(f.values.len() as u32);
    f.values.push(ValueInfo {
        ty: Type::Bool,
        name: None,
    });
    body.push(inst(
        odd,
        InstKind::ICmp(IPred::Ne, Operand::Value(t), Operand::ConstI64(0)),
    ));
    f.blocks[2].insts = body;
    f.blocks[2].term = Terminator::CondBr {
        cond: Operand::Value(odd),
        then_blk: BlockId(4),
        else_blk: BlockId(5),
    };
    f.blocks.push(Block {
        id: BlockId(4),
        insts: write,
        term: Terminator::Br(BlockId(5)),
    });
    f.blocks.push(Block {
        id: BlockId(5),
        insts: vec![],
        term: Terminator::Br(BlockId(1)),
    });
    let (f, counts) = run(f);
    assert_eq!(counts, [1, 0, 1, 0]);
    let write = &f.blocks[4].insts;
    assert_eq!(write.len(), 4);
    let (_, add) = step_at(write);
    assert_eq!(add, 2);
    let fadd = |i: &Inst| matches!(i.kind, InstKind::Vec(VecInst::BinOp(VecOp::FAdd, ..)));
    assert!(!f.blocks[2].insts.iter().any(fadd));
}

#[test]
fn a_write_between_the_conversion_and_the_divide_is_splatted() {
    let mut f = counters(1, false);
    // `y = sitofp d; d = d + 1; fdiv a, y`: a cell stepped with the
    // write would hand the divide the new `d`
    let body = &mut f.blocks[2].insts;
    let step: Vec<Inst> = body.drain(body.len() - 2..).collect();
    body.splice(1..1, step);
    let (f, counts) = run(f);
    assert_eq!(counts, [1, 0, 0, 0]);
    assert!(
        f.blocks[2]
            .insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::Vec(VecInst::Splat(VecShape::F64x2, _))))
    );
}
