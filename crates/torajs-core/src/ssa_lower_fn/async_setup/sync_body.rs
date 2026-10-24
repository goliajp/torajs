//! An async function whose body never awaits (`ast::body_awaits`):
//! ECMA-262 §27.7.5.2 AsyncBlockStart runs the body to its end inside
//! the call, so there is nothing to suspend and no frame. The body is
//! lowered as the function itself, returning what settles its result
//! (`suspend_ret`), and each of its ways out then makes the result
//! (torajs-promise `frame::sync`): a return settles it with the
//! returned value (§27.7.5.2 step 3.d-f, adopting a thenable through
//! §27.2.1.3.2), a throw the body does not catch rejects it with the
//! throw (step 3.g; a parameter default that throws included, §15.8.4
//! step 3). The function itself never raises.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk;
use crate::ast::{Ast, Stmt};
use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower_promise_repr_mark as mark;
use crate::ssa_lower_synthesize_main::declare_intrinsic;

/// The runtime calls of a body that never awaits.
#[derive(Debug, Clone, Copy)]
struct SyncCalls {
    resolved: FuncId,
    rejected: FuncId,
    /// The fulfilled-cell mints `Promise.resolve` of a primitive uses.
    fulfilled: FuncId,
    fulfilled_heap: FuncId,
}

/// The calls that take the pending throw: its tag (which leaves the
/// throw pending) and then its value (which clears it).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThrowTake {
    pub(crate) tag: FuncId,
    pub(crate) value: FuncId,
}

/// The moved async functions whose bodies never await, and the calls
/// they make (declared only when there is one).
#[derive(Default)]
pub(crate) struct SyncBodies {
    names: HashSet<String>,
    calls: Option<SyncCalls>,
}

impl SyncBodies {
    /// Take the bodies that never await out of `names`.
    pub(crate) fn reserve(
        ast: &Ast,
        names: &mut Vec<&String>,
        module: &mut Module,
        fn_table: &mut HashMap<String, FuncId>,
    ) -> Self {
        let sync = never_await(ast, names);
        names.retain(|n| !sync.contains(n.as_str()));
        let fulfilled = fn_table[mark::ALLOC_FULFILLED_REPR];
        let fulfilled_heap = fn_table[mark::ALLOC_FULFILLED_HEAP_REPR];
        let mut decl = |name: &str, params: &[Type]| {
            declare_intrinsic(module, fn_table, name, params, Type::Promise)
        };
        let calls = (!sync.is_empty()).then(|| SyncCalls {
            resolved: decl("__torajs_async_resolved", &[Type::I64, Type::I64]),
            rejected: decl("__torajs_async_rejected", &[Type::I64, Type::I64]),
            fulfilled,
            fulfilled_heap,
        });
        SyncBodies { names: sync, calls }
    }

    pub(crate) fn holds(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// The lowered body `f` of one of them as the function callers
    /// reach ([`settle_in_place`]).
    pub(crate) fn settle_in_place(
        &self,
        f: ssa::Function,
        repr: i64,
        take: ThrowTake,
    ) -> ssa::Function {
        let calls = self.calls.expect("a body that never awaits has its calls");
        settle_in_place(f, calls, take, repr)
    }
}

/// Which of `names` (async functions that moved off the state machine)
/// have a body with no await of its own.
fn never_await(ast: &Ast, names: &[&String]) -> HashSet<String> {
    let wanted: HashSet<&str> = names.iter().map(|n| n.as_str()).collect();
    let mut bodies: HashMap<&str, &[Stmt]> = HashMap::new();
    fn collect<'a>(s: &'a Stmt, wanted: &HashSet<&str>, out: &mut HashMap<&'a str, &'a [Stmt]>) {
        if let Stmt::FnDecl { name, body, .. } = s
            && wanted.contains(name.as_str())
        {
            out.insert(name.as_str(), body.as_slice());
        }
        for c in walk::stmt_children_ref(s) {
            collect(c, wanted, out);
        }
    }
    for s in &ast.stmts {
        collect(s, &wanted, &mut bodies);
    }
    let body_of = |n: &str| {
        *bodies
            .get(n)
            .unwrap_or_else(|| panic!("internal error: async function `{n}` has no declaration"))
    };
    (names.iter())
        .filter(|n| !crate::ast::body_awaits(ast, body_of(n)))
        .map(|n| n.to_string())
        .collect()
}

/// The lowered body as the function callers reach: every return makes
/// the result instead, settled with the value the body returned in the
/// form `repr` names, or rejected where the body propagates a throw.
///
/// A propagating block takes the throw before it rejects, the way a
/// catch does, and returns as any other: the function does not signal,
/// and the codegen's throw register, which a reader zeroes right after
/// `__torajs_throw_take` (torajs-codegen `compile/throw_reg.rs`), is 0
/// again when the call returns. Taking it in the runtime instead would
/// leave the register set with no throw pending, and the caller's next
/// probe of a signalling call would see a throw that is not there.
///
/// Any other return is reached with no throw pending, so it settles
/// without asking: the front end probes every call that may throw and
/// sends a pending throw to a propagating block or to a catch's take,
/// the judgement every function's normal return already rests on. The
/// frame lane's `__torajs_frame_return` asks only because it shares the
/// settle of a reaction handler, which the runtime calls unprobed.
fn settle_in_place(
    mut f: ssa::Function,
    calls: SyncCalls,
    take: ThrowTake,
    repr: i64,
) -> ssa::Function {
    let ret_ty = f.ret;
    for bi in 0..f.blocks.len() {
        let Terminator::Ret(v) = f.blocks[bi].term.clone() else {
            continue;
        };
        let b = ssa::BlockId(bi as u32);
        let insts = &mut f.blocks[bi].insts;
        let call = match (insts.iter()).position(|i| matches!(i.kind, InstKind::ThrowSignal)) {
            Some(at) => {
                insts.remove(at);
                let tag = push(&mut f, b, InstKind::Call(take.tag, Vec::new()), Type::I64);
                let value = push(&mut f, b, InstKind::Call(take.value, Vec::new()), Type::I64);
                let pair = vec![Operand::Value(tag), Operand::Value(value)];
                InstKind::Call(calls.rejected, pair)
            }
            None => {
                let bits = match v {
                    Some(op) => value_bits(&mut f, b, op, ret_ty),
                    None => Operand::ConstI64(0),
                };
                let r = Operand::ConstI64(repr);
                // a primitive is neither adopted nor probed for a `then`:
                // the result is a fulfilled cell, minted as one
                match repr {
                    mark::REPR_HEAP | mark::REPR_ANY => {
                        InstKind::Call(calls.resolved, vec![r, bits])
                    }
                    mark::REPR_STR => InstKind::Call(calls.fulfilled_heap, vec![bits, r]),
                    _ => InstKind::Call(calls.fulfilled, vec![bits, r]),
                }
            }
        };
        let result = push(&mut f, b, call, Type::Promise);
        f.blocks[bi].term = Terminator::Ret(Some(Operand::Value(result)));
    }
    f.ret = Type::Promise;
    f
}

/// The returned value as the i64 the result's value word holds
/// (`ssa::suspend::value_word`).
fn value_bits(f: &mut ssa::Function, b: ssa::BlockId, op: Operand, ty: Type) -> Operand {
    match ssa::suspend::value_word(op, ty) {
        ssa::suspend::ValueWord::Ready(word) => word,
        ssa::suspend::ValueWord::Convert(kind) => Operand::Value(push(f, b, kind, Type::I64)),
    }
}

/// Append to a block whose return is already set.
fn push(f: &mut ssa::Function, b: ssa::BlockId, kind: InstKind, ty: Type) -> ssa::ValueId {
    let at = f.blocks[b.0 as usize].insts.len();
    f.insert_inst(b, at, kind, ty)
}
