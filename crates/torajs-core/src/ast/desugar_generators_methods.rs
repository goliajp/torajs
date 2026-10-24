//! Generator class method emit helpers.
//!
//! P10.6-A1 — extracted from `ast::desugar_generators` so the
//! ~478-LOC desugar fn stays within touching distance of the
//! file-size hard limit even as P10.6 layers new Generator
//! prototype methods (`.return`, `.throw`, future iterator-
//! protocol surface).

use super::{Ast, ClassMethod, Expr, ExprId, Param, Stmt, Visibility};

const RET_VAL_PARAM: &str = "__ret_val";
const THROW_ERR_PARAM: &str = "__err";
const YIELD_ARG_PARAM: &str = "__yield_arg";

/// Emit `this.__state = -1;` — the sentinel that turns any
/// subsequent `next()` into a no-op (no state-machine arm
/// matches; control falls through to the tail
/// `{ value: <default>, done: true }`). Shared by `.return` and
/// `.throw` since both close the generator the same way.
fn emit_close_state(ast: &mut Ast) -> Stmt {
    let this_id = ast.add_expr(Expr::This);
    let state_member = ast.add_expr(Expr::Member {
        obj: this_id,
        name: "__state".into(),
    });
    let neg_one = ast.add_expr(Expr::Number(-1.0));
    let assign_state = ast.add_expr(Expr::Assign {
        target: state_member,
        value: neg_one,
    });
    Stmt::Expr(assign_state)
}

/// `this.next(<the same default `next` itself declares>)` — the
/// internal re-entry the region-bearing `.return` / `.throw` shapes
/// use to resume the state machine.
///
/// The argument is passed EXPLICITLY rather than left to
/// `apply_default_args`. That pass keys class-method defaults by bare
/// name whenever it cannot resolve the receiver, and `this` inside a
/// synthesized `__cm_*` body is exactly such a receiver — so a
/// program containing any second owner of the name `next` (a user
/// `class Cursor { next(step = 5) {} }`, or simply a generator on the
/// other yield-type lane) evicted the shared entry and left this call
/// unpadded: "expected 1 argument(s), got 0", reported against a call
/// the compiler wrote itself. The value handed over is identical
/// either way; what goes away is the dependency on a global table.
fn emit_self_next_call(ast: &mut Ast, sent_ty: &str, next_name: &str) -> ExprId {
    let arg = ast.add_expr(yield_arg_undefined(sent_ty));
    let this_recv = ast.add_expr(Expr::This);
    let next_callee = ast.add_expr(Expr::Member {
        obj: this_recv,
        name: next_name.into(),
    });
    ast.add_expr(Expr::Call {
        callee: next_callee,
        args: vec![arg],
    })
}

/// Build the `Generator.prototype.return(value)` method per ES
/// spec §27.5.1.7. Writes a sentinel `__state = -1` so any
/// subsequent `next()` falls through every state arm to the
/// `{ value: <default>, done: true }` tail, then returns
/// `{ value: arg, done: true }` directly.
///
/// RFC 20260802 D3b — a generator with a return-injectable finally
/// region (`has_finally_ret`) instead delegates, the dual of the C2
/// throw() shape: `this.__ret_inj = v; this.__ret_pending = true;
/// return this.next();`. next()'s dispatch return-check routes a
/// suspended-in-finally-region state to that region's D3a return
/// copy (F runs — may yield — then completes with the stashed
/// value, chaining outer frames), and completes directly otherwise
/// — same caller-visible shape as the close path.
///
/// Generators without such a region keep the close+return shape: no
/// finally can observe the abrupt completion, and skipping the
/// delegate keeps their class free of the `__ret_*` fields.
pub(super) fn build_return_method(
    ast: &mut Ast,
    (yield_ty, sent_ty): (&str, &str),
    step_ann: &str,
    has_finally_ret: bool,
    is_async_gen: bool,
    next_name: &str,
) -> ClassMethod {
    let val_param = Param {
        name: RET_VAL_PARAM.into(),
        type_ann: Some(yield_ty.into()),
        default: None,
        is_rest: false,
    };
    let body = if has_finally_ret {
        let this_id = ast.add_expr(Expr::This);
        let inj = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__ret_inj".into(),
        });
        let val_ident = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let stash = ast.add_expr(Expr::Assign {
            target: inj,
            value: val_ident,
        });
        let this_arm = ast.add_expr(Expr::This);
        let pending = ast.add_expr(Expr::Member {
            obj: this_arm,
            name: "__ret_pending".into(),
        });
        let true_lit = ast.add_expr(Expr::Bool(true));
        let arm = ast.add_expr(Expr::Assign {
            target: pending,
            value: true_lit,
        });
        let call = emit_self_next_call(ast, sent_ty, next_name);
        vec![Stmt::Expr(stash), Stmt::Expr(arm), Stmt::Return(Some(call))]
    } else {
        let close_stmt = emit_close_state(ast);
        let val_ident = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let done_true = ast.add_expr(Expr::Bool(true));
        let result = ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val_ident), ("done".into(), done_true)],
        });
        vec![close_stmt, Stmt::Return(Some(result))]
    };
    // D4 — an ASYNC generator's return(v) reaches this method as the
    // driver's `__step_return`, v already awaited (§27.6.3.7 step 8.d,
    // AwaitReturn — `__agen_run` awaits a return request's value before
    // stepping). While the body is delegating (`this.__ysact`, set by
    // `desugar_generators_yieldstar`'s loop) the request is a
    // RETURN-completion resumption of the delegating yield (§27.5.3.2
    // step 7.c forwards it to the inner iterator): flag it and resume
    // as a next(v); the loop reads the flag off `this.__ysret` after
    // the yield and takes the return arm.
    let delegating = if is_async_gen {
        let this_a = ast.add_expr(Expr::This);
        let act = ast.add_expr(Expr::Member {
            obj: this_a,
            name: super::desugar_generators_async::YS_ACTIVE_FIELD.into(),
        });
        let this_b = ast.add_expr(Expr::This);
        let ret_flag = ast.add_expr(Expr::Member {
            obj: this_b,
            name: super::desugar_generators_async::YS_RETURN_FIELD.into(),
        });
        let t = ast.add_expr(Expr::Bool(true));
        let set_flag = ast.add_expr(Expr::Assign {
            target: ret_flag,
            value: t,
        });
        let this_c = ast.add_expr(Expr::This);
        let next_callee = ast.add_expr(Expr::Member {
            obj: this_c,
            name: next_name.into(),
        });
        let v = ast.add_expr(Expr::Ident(RET_VAL_PARAM.into()));
        let resume = ast.add_expr(Expr::Call {
            callee: next_callee,
            args: vec![v],
        });
        Some(Stmt::If {
            cond: act,
            then_branch: Box::new(Stmt::Block(vec![
                Stmt::Expr(set_flag),
                Stmt::Return(Some(resume)),
            ])),
            else_branch: None,
        })
    } else {
        None
    };
    let body: Vec<Stmt> = delegating.into_iter().chain(body).collect();
    ClassMethod {
        name: "return".into(),
        type_params: Vec::new(),
        params: vec![val_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// Build the `Generator.prototype.throw(err)` method per ES spec
/// §27.5.1.4. Force-closes the generator (same `__state = -1`
/// sentinel as `.return`) and rethrows `err` to the caller via
/// `Stmt::Throw`. The throw substrate (`torajs_throw_set` +
/// `emit_throw_check`) propagates the error to the innermost
/// enclosing try/catch in the caller's frame.
///
/// RFC 20260802 C2 — a REGION-BEARING generator (`has_try_regions`)
/// instead injects: `this.__thrown = err; this.__inject = true;
/// return this.next();`. next()'s prologue throws the stash at the
/// suspended state, so a try wrapping the suspended yield observes
/// it (§27.5.1.4 resume-with-throw-completion); a state in no region
/// rethrows out of next() — same caller-visible shape as the close
/// path, with the prologue kill supplying the "completed" sentinel.
///
/// Region-FREE generators keep the close+rethrow shape: no body try
/// can observe the error, and skipping injection keeps their class
/// free of the `__inject` / `__thrown` fields.
pub(super) fn build_throw_method(
    ast: &mut Ast,
    sent_ty: &str,
    step_ann: &str,
    has_try_regions: bool,
    next_name: &str,
) -> ClassMethod {
    let err_param = Param {
        name: THROW_ERR_PARAM.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    };
    let body = if has_try_regions {
        let this_id = ast.add_expr(Expr::This);
        let thrown = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__thrown".into(),
        });
        let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
        let stash = ast.add_expr(Expr::Assign {
            target: thrown,
            value: err_ident,
        });
        let this_arm = ast.add_expr(Expr::This);
        let inj = ast.add_expr(Expr::Member {
            obj: this_arm,
            name: "__inject".into(),
        });
        let true_lit = ast.add_expr(Expr::Bool(true));
        let arm = ast.add_expr(Expr::Assign {
            target: inj,
            value: true_lit,
        });
        let call = emit_self_next_call(ast, sent_ty, next_name);
        vec![Stmt::Expr(stash), Stmt::Expr(arm), Stmt::Return(Some(call))]
    } else {
        let close_stmt = emit_close_state(ast);
        let err_ident = ast.add_expr(Expr::Ident(THROW_ERR_PARAM.into()));
        vec![close_stmt, Stmt::Throw(err_ident)]
    };
    ClassMethod {
        name: "throw".into(),
        type_params: Vec::new(),
        params: vec![err_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// RFC 20260805-generator-value-channels, the caller → body half.
/// The type of the resume channel — the `next(__yield_arg)`
/// parameter, the `__sent` field it is stashed in, and the `let v =
/// this.__sent` a `yield` expression reads — for a generator whose
/// yield type is `yield_ty`. §27.5.1.1.1 sends `undefined` for a bare
/// `next()`, and a `T` slot cannot hold it; `T | undefined` can, and
/// is what the parser already gives a `p?: T` parameter: a scalar
/// materializes as `any` (ANY_UNDEF), a pointer-shaped slot keeps its
/// per-type sentinel cell. TypeScript types the value of a `yield`
/// expression `any` unless the generator declares `TNext`, so the
/// widening costs the program nothing it was promised. Before this
/// the channel was `T` end to end and a bare `next()` sent `T`'s
/// zero: `const a = yield 1` read `0`.
pub(super) fn sent_slot_ann(yield_ty: &str) -> String {
    if matches!(yield_ty, "any" | "void" | "undefined") || yield_ty.starts_with("__nullable(") {
        return yield_ty.to_string();
    }
    format!("__nullable({yield_ty})")
}

/// RFC 20260805, the body → caller half: the completion value of an
/// exhausted generator (`{value: undefined, done: true}`, §27.5.1.2 —
/// and so the result of `yield*` over it), spelled the way the step's
/// `value: <yield_ty>` slot holds it WITHOUT widening the slot: the
/// plain ident for an `any`-shaped slot, otherwise the
/// `__undef_slot__<T>` marker, which types as `T` and lowers to that
/// width's own `undefined` — the F64 sentinel bits for `number` (the
/// spelling a `number[]` read past the end answers), the per-type
/// sentinel cell for a pointer-shaped `T`. The yield path is untouched
/// (design principle 1: the blunt fix, boxing the step's value, cost
/// +14.7% on `generator-yield-100k`); the consumers that tell
/// `undefined` from a number are the ones the sentinel already has
/// (`is_undef_f64_source`, `undef_f64_fields`). Before this the
/// completion value was `T`'s zero: `yield*` answered `0`.
pub(super) fn completion_undefined(yield_ty: &str) -> Expr {
    let inner = yield_ty
        .strip_prefix("__nullable(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(yield_ty);
    if crate::ssa_lower_parse_type::slot_is_any(inner) {
        return Expr::Ident("undefined".into());
    }
    Expr::Ident(format!("{}{inner}", crate::ast::UNDEF_SLOT_MARKER))
}

/// The value a bare `next()` sends, spelled the way [`sent_slot_ann`]'s
/// slot holds it — the rule `parser::param_optional_default` fixes for
/// `p?: T`: the plain ident for a slot that materializes as `any`
/// (the any lane, and a `number` / `boolean` `T | undefined`), the
/// `__undef_slot__<T>` marker for a pointer-shaped one, which asks the
/// SSA lowering for that type's sentinel cell. Every lane whose slot
/// is `any`-shaped spells the same ident, so `apply_default_args`' by-
/// name table keeps one `next` entry across them — the constraint RFC
/// 20260805 records (rotations 305 / 306 evicted it by moving one lane
/// alone). Before this the default was the yield type's ZERO (`0` for
/// the any lane too, to stay uniform).
pub(super) fn yield_arg_undefined(yield_ty: &str) -> Expr {
    let inner = yield_ty
        .strip_prefix("__nullable(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(yield_ty);
    if crate::ssa_lower_parse_type::nullable_inner_boxes(inner)
        || crate::ssa_lower_parse_type::slot_is_any(inner)
    {
        return Expr::Ident("undefined".into());
    }
    Expr::Ident(format!("{}{inner}", crate::ast::UNDEF_SLOT_MARKER))
}

/// Build the `Generator.prototype.next(arg)` method. Prepends a
/// `this.__sent = __yield_arg;` stash so YieldInto-expanded reads
/// of `this.__sent` see whatever the caller passed to `g.next(arg)`
/// on the resume; the rest of the method body is the caller-built
/// state-machine `while(true) { ... }` + tail return.
///
/// `yield_arg_default_id` carries the resume default — `undefined`
/// in the channel's own spelling (`yield_arg_undefined`), minted by
/// the caller so it can forward one ExprId.
/// Carry a generator body's strictness onto the `next()` the state
/// machine becomes. §11.2.2 makes the body strict code, and the
/// directive prologue is the only carrier that survives this far
/// (`parser::strict_directive`) — but `build_state_machine_next_body`
/// lowers the whole body through `GenSm`, so a directive at its head
/// ends up inside `while (true) { if (st == 0) { … } }`, where the
/// per-function probe that reads it
/// (`ssa_lower_set_flavor::cur_fn_is_sloppy`) cannot see it. Every
/// generator therefore answered "sloppy", and a refused [[Set]] in a
/// strict one ran through.
///
/// Re-minted rather than moved: the original statement is a pure
/// string expression, harmless where the state machine left it, and
/// moving it would renumber the arms it was lowered into.
pub(super) fn carry_strictness_to_next(ast: &mut Ast, next: &mut ClassMethod, strict: bool) {
    if !strict {
        return;
    }
    let e = crate::ast::sloppy_this_prologue::mint_use_strict_directive(ast);
    next.body.insert(0, Stmt::Expr(e));
}

pub(super) fn build_next_method(
    ast: &mut Ast,
    yield_arg_default_id: ExprId,
    sent_ty: &str,
    step_ann: &str,
    state_machine_body: Vec<Stmt>,
) -> ClassMethod {
    let yield_arg_param = Param {
        name: YIELD_ARG_PARAM.into(),
        type_ann: Some(sent_slot_ann(sent_ty)),
        default: Some(yield_arg_default_id),
        is_rest: false,
    };
    let stash_sent = {
        let this_id = ast.add_expr(Expr::This);
        let sent_member = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__sent".into(),
        });
        let arg_ident = ast.add_expr(Expr::Ident(YIELD_ARG_PARAM.into()));
        let assign = ast.add_expr(Expr::Assign {
            target: sent_member,
            value: arg_ident,
        });
        Stmt::Expr(assign)
    };
    let mut body: Vec<Stmt> = Vec::with_capacity(state_machine_body.len() + 1);
    // Only a body that READS the channel pays for the stash: the slot
    // is `T | undefined` — an `any` for a scalar `T` — so the store is
    // an rc-accounted field write on every resume, and
    // `generator-yield-100k` (a `yield i` statement, nothing reads what
    // `next()` sends) measured +12% run_ms with it unconditional
    // (rotation 640, same-round A/B, bun-aot control flat). The
    // parameter itself is a +0 borrow the callee never touches.
    if body_reads_sent(ast, &state_machine_body) {
        body.push(stash_sent);
    }
    body.extend(state_machine_body);
    ClassMethod {
        name: "next".into(),
        type_params: Vec::new(),
        params: vec![yield_arg_param],
        return_type: Some(step_ann.into()),
        body,
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

/// Does the state machine read `this.__sent` anywhere — a `yield`
/// used as an expression (`expand_yield_into_in_stmt`'s `let v =
/// this.__sent`)? Every statement list the body owns, every
/// expression under it.
pub(super) fn body_reads_sent(ast: &Ast, body: &[Stmt]) -> bool {
    use crate::ast::desugar_with::walk;
    fn expr_reads(ast: &Ast, eid: ExprId) -> bool {
        if let Expr::Member { obj, name } = ast.get_expr(eid)
            && name == "__sent"
            && matches!(ast.get_expr(*obj), Expr::This)
        {
            return true;
        }
        walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_reads(ast, c))
    }
    fn stmt_reads(ast: &Ast, s: &Stmt) -> bool {
        walk::stmt_exprs(s).into_iter().any(|e| expr_reads(ast, e))
            || walk::stmt_children_ref(s)
                .into_iter()
                .any(|c| stmt_reads(ast, c))
    }
    body.iter().any(|s| stmt_reads(ast, s))
}
