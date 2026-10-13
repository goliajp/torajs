//! A small interpreter for the subset the tests build, so a jammed
//! function can be run against its original. An `f64` value is held as
//! its bits; a vector as its two lanes' bits.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst,
    VecOp,
};

type Lanes = [i64; 2];

/// A vector instruction's lanes, or (`Err`) its scalar result.
fn vec_op(
    env: &HashMap<ValueId, i64>,
    venv: &HashMap<ValueId, Lanes>,
    vi: &VecInst,
) -> Result<Lanes, i64> {
    let lanes = |op: &Operand| match op {
        Operand::Value(x) => venv[x],
        other => panic!("vector operand {other:?}"),
    };
    let fl = |x: i64| f64::from_bits(x as u64);
    let mask = |b: bool| if b { -1 } else { 0 };
    let per_lane = |op: VecOp, a: Lanes, b: Lanes| -> Lanes {
        let one = |x: i64, y: i64| match op {
            VecOp::Add => x.wrapping_add(y),
            VecOp::Sub => x.wrapping_sub(y),
            VecOp::And => x & y,
            VecOp::AndNot => x & !y,
            VecOp::Or => x | y,
            VecOp::FAdd => (fl(x) + fl(y)).to_bits() as i64,
            VecOp::FSub => (fl(x) - fl(y)).to_bits() as i64,
            VecOp::FMul => (fl(x) * fl(y)).to_bits() as i64,
            VecOp::FCmEq => mask(fl(x) == fl(y)),
            VecOp::FCmGt => mask(fl(x) > fl(y)),
            VecOp::FCmGe => mask(fl(x) >= fl(y)),
            VecOp::CmEq => mask(x == y),
            VecOp::CmGt => mask(x > y),
            VecOp::CmGe => mask(x >= y),
            VecOp::FDiv => panic!("fdiv outside the test subset"),
        };
        [one(a[0], b[0]), one(a[1], b[1])]
    };
    Ok(match vi {
        VecInst::Splat(_, a) => [operand(env, a); 2],
        VecInst::Pair(_, a, b) => [operand(env, a), operand(env, b)],
        VecInst::BinOp(op, a, b) => per_lane(*op, lanes(a), lanes(b)),
        VecInst::Select(m, a, b) => {
            let (m, a, b) = (lanes(m), lanes(a), lanes(b));
            [
                if m[0] != 0 { a[0] } else { b[0] },
                if m[1] != 0 { a[1] } else { b[1] },
            ]
        }
        VecInst::Ext(a, b) => [lanes(a)[1], lanes(b)[0]],
        VecInst::AnyLane(a) => return Err(lanes(a)[0] | lanes(a)[1]),
        VecInst::Lane(k, a) => return Err(lanes(a)[*k as usize]),
        other => panic!("{other:?} outside the test subset"),
    })
}

fn operand(env: &HashMap<ValueId, i64>, op: &Operand) -> i64 {
    match op {
        Operand::Value(x) => env[x],
        Operand::ConstI64(k) => *k,
        Operand::ConstBool(b) => *b as i64,
        Operand::ConstF64(x) => x.to_bits() as i64,
        other => panic!("operand {other:?} outside the test subset"),
    }
}

fn float(env: &HashMap<ValueId, i64>, op: &Operand) -> f64 {
    f64::from_bits(operand(env, op) as u64)
}

fn float_op(env: &HashMap<ValueId, i64>, kind: &InstKind) -> Option<i64> {
    let r = match kind {
        InstKind::BinOp(op, a, b) => {
            let (x, y) = (float(env, a), float(env, b));
            match op {
                BinOp::FAdd => x + y,
                BinOp::FSub => x - y,
                BinOp::FMul => x * y,
                _ => return None,
            }
        }
        InstKind::SiToFp(a) => operand(env, a) as f64,
        InstKind::FCmp(p, a, b) => {
            let (x, y) = (float(env, a), float(env, b));
            let r = match p {
                FPred::Olt => x < y,
                FPred::Ogt => x > y,
                FPred::Oge => x >= y,
                other => panic!("{other:?} outside the test subset"),
            };
            return Some(r as i64);
        }
        _ => return None,
    };
    Some(r.to_bits() as i64)
}

/// Run `f` on `args`; `None` when it does not return within `fuel`
/// instructions.
pub(super) fn run(f: &Function, args: &[i64], fuel: usize) -> Option<i64> {
    let mut env: HashMap<ValueId, i64> =
        f.params.iter().copied().zip(args.iter().copied()).collect();
    let mut venv: HashMap<ValueId, Lanes> = HashMap::default();
    let mut b = BlockId(0);
    let mut left = fuel;
    loop {
        let block = &f.blocks[b.0 as usize];
        for i in &block.insts {
            left = left.checked_sub(1)?;
            let result = i.result.expect("value insts");
            match &i.kind {
                InstKind::Copy(Type::Vec(_), Operand::Value(a)) => {
                    venv.insert(result, venv[a]);
                    continue;
                }
                InstKind::Vec(vi) => {
                    match vec_op(&env, &venv, vi) {
                        Ok(lanes) => {
                            venv.insert(result, lanes);
                        }
                        Err(x) => {
                            env.insert(result, x);
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let r = match &i.kind {
                kind if float_op(&env, kind).is_some() => float_op(&env, kind).expect("float"),
                InstKind::Copy(_, a) => operand(&env, a),
                InstKind::Select(_, c, a, bb) => {
                    if operand(&env, c) != 0 {
                        operand(&env, a)
                    } else {
                        operand(&env, bb)
                    }
                }
                InstKind::BinOp(op, a, bb) => {
                    let (x, y) = (operand(&env, a), operand(&env, bb));
                    match op {
                        BinOp::Add => x.wrapping_add(y),
                        BinOp::Sub => x.wrapping_sub(y),
                        BinOp::Mul => x.wrapping_mul(y),
                        BinOp::SRem => x.checked_rem(y).unwrap_or(x),
                        BinOp::And => x & y,
                        BinOp::AShr => x >> (y & 63),
                        other => panic!("{other:?} outside the test subset"),
                    }
                }
                InstKind::ICmp(p, a, bb) => {
                    let (x, y) = (operand(&env, a), operand(&env, bb));
                    (match p {
                        IPred::Eq => x == y,
                        IPred::Ne => x != y,
                        IPred::Slt => x < y,
                        IPred::Sle => x <= y,
                        IPred::Sgt => x > y,
                        IPred::Sge => x >= y,
                        other => panic!("{other:?} outside the test subset"),
                    }) as i64
                }
                other => panic!("{other:?} outside the test subset"),
            };
            env.insert(result, r);
        }
        b = match &block.term {
            Terminator::Br(t) => *t,
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                if operand(&env, cond) != 0 {
                    *then_blk
                } else {
                    *else_blk
                }
            }
            Terminator::Ret(r) => return Some(operand(&env, r.as_ref().expect("ret value"))),
            Terminator::Unreachable => panic!("reached unreachable"),
        };
    }
}
