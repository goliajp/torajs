//! A `let` / `const` whose closure initializer needs the binding to
//! already exist — either because the closure names the binding itself
//! (`const f = function (n) { … f(n - 1) … }`) or because a peer
//! declared earlier in the same statement list names it (two arrows
//! that call each other).
//!
//! Every other init lowers value-first: produce the operand, then give
//! the binding a slot to hold it. That order cannot work here. The
//! closure's env has to hold the binding at mint time, and the binding
//! has nothing to hold until the mint is done. So this lane inverts it:
//!
//! 1. Mint the capture box holding null and register the binding on it.
//!    Recording the name in `boxed_noncopy_lets` is what makes the
//!    capture write take the byref path — the env stores the BOX
//!    pointer and takes a share of the box, rather than snapshotting a
//!    value that does not exist yet.
//! 2. Lower the closure. Its capture write now finds the box.
//! 3. Store the closure into the box. Every read of the name, including
//!    the ones inside the body, goes through that single cell — which
//!    is also what ES §9.1 asks for: a closure captures the BINDING.
//!
//! Step 1 splits by which shape brought us here. Self-reference is
//! visible from the declaration alone, so [`try_lower`] does it inline.
//! A forward reference is not — the statement that needs the box comes
//! BEFORE the one that declares it — so [`hoist_forward_boxes`] runs
//! over the whole list first and leaves the box waiting; `try_lower`
//! then finds it and goes straight to steps 2 and 3.
//!
//! The env holds the box and the box holds the env: a genuine reference
//! cycle, and precisely the one the collector exists for. A byref
//! non-Copy slot answers `cap_is_traceable`, so the mint emits a real
//! `__env_trace_<fn>` and `collect_white` can walk in and break the
//! edge (RFC 20260717 closure-env-cycle). Refcounting alone will never
//! reclaim a self-referential closure — that is a property of the
//! shape, not a gap in this lane.
//!
//! Narrow by construction: only closure initializers, and only when the
//! binding takes a `Closure` slot. An `any`-annotated binding takes an
//! any slot instead, and the checker declines it in step with this.

use std::collections::HashSet;

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

/// Give a binding a capture box holding null, register it, and answer
/// the box's value-slot pointer. Shared by both entry shapes so the
/// ownership bookkeeping is written once.
fn open_box(ctx: &mut LowerCtx, name: &str, ty: Type) -> crate::ssa::ValueId {
    let cur_depth = ctx.scope_stack.len() - 1;
    let slot = ctx.emit_capture_boxed(ty, Operand::ConstPtrNull);
    ctx.boxed_noncopy_lets.insert(name.to_string());
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
        top_shadow.push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            // The box owns the closure's stake and this frame owns the
            // box, so the scope-close walk has to reach it (the
            // `boxed_noncopy_lets` arm of the drop walk releases it).
            moved: true,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    ctx.scope_stack
        .last_mut()
        .expect("scope frame")
        .push(name.to_string());
    slot
}

/// Open `name`'s capture box HERE, in the ordinary-binding shape
/// [`hoist_forward_boxes`] gives a forward-referenced one: a
/// provisional `Any` box the declaration patches when it runs.
///
/// The switch lowerer needs the same thing for a different reason.
/// §14.12.4 gives the whole CaseBlock one declarative environment, so
/// a closure in one clause may capture a binding another clause
/// declares — and the clause bodies are sibling basic blocks. A box
/// minted where the declaration sits is defined in a block that does
/// not dominate the capture, and a constant scrutinee
/// (`switch (1) { case 0: let x; case 1: (function () { x; })(); }`)
/// deletes that block outright, leaving the capture reading a value
/// with no definition ("ValueId not allocated" at regalloc). Minting
/// it before the compare chain puts it where every clause can see it.
pub(crate) fn open_case_block_box(ctx: &mut LowerCtx, name: &str) {
    if ctx.is_main_fn && ctx.globals.contains_key(name) {
        return;
    }
    if ctx.hoisted_closure_lets.contains(name) || ctx.forward_capture_boxes.contains_key(name) {
        return;
    }
    open_box(ctx, name, Type::Any);
    ctx.forward_capture_boxes
        .insert(name.to_string(), Vec::new());
}

/// The binding's slot type: an annotated binding keeps its declared
/// signature — that is what the call sites type against — and an
/// inferred one takes exactly what the mint is about to produce, read
/// off the same function the mint itself reads so the box and its
/// content cannot disagree. `None` when the binding takes neither a
/// closure slot nor an any slot.
///
/// An `any` slot answers here too. It did not before, and the two
/// halves of that asymmetry were visible from the source: `let a: any
/// = [function () { … a … }]` worked (the nested-closure walk below
/// plans it as an ordinary binding, whose arm opens an `Any` box),
/// while the same closure written BARE — `let a: any = function () {
/// … a … }` — reached this gate, was declined, and died at the mint
/// with `closure capture not in scope`. Wrapping the closure in an
/// array was the whole difference. The box the ordinary arm opens is
/// an `Any` box already, so admitting the type here is the same
/// mechanism reaching the same shape by its own route.
fn box_ty(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    fn_name: &str,
    mutable: bool,
) -> Option<Type> {
    let ty = if type_ann.is_some() {
        crate::ssa_lower_stmt_let_decl_general::initial_let_ty(ctx, name, type_ann, init, mutable)
    } else {
        crate::ssa_lower_closure::closure_value_ty(ctx, fn_name)
    };
    matches!(ty, Type::Closure(_) | Type::Any).then_some(ty)
}

/// Open the box for every binding in `stmts` that a closure declared
/// EARLIER in the same list captures. Runs once per statement list,
/// before any of it lowers, so a mutually recursive pair finds each
/// other; `try_lower` recognizes the waiting box by the name being in
/// `hoisted_closure_lets`.
///
/// Only earlier captures: a self-capture is `try_lower`'s inline case,
/// and a LATER peer capturing an EARLIER binding needs nothing — by
/// then the binding is ordinary and already there.
///
/// The captured binding does not have to be a closure itself. Whatever
/// `const o = {v:3}` holds, a closure declared above it captures the
/// BINDING (ES §9.1), so the box has to exist by the time that closure
/// mints — the two shapes differ only in whether the slot type is
/// knowable this early, and the ordinary one settles that later.
pub(crate) fn hoist_forward_boxes<'s>(ctx: &mut LowerCtx, stmts: impl Iterator<Item = &'s Stmt>) {
    let mut seen_captures: HashSet<String> = HashSet::new();
    let mut plan: Vec<Planned> = Vec::new();
    collect(ctx, stmts, &mut seen_captures, &mut plan);
    for p in plan {
        let Planned {
            name,
            type_ann,
            init,
            fn_name,
            mutable,
        } = p;
        let Some(fn_name) = fn_name else {
            // An ordinary binding. A top-level one is a data global,
            // which needs no box at all — the closure resolves the name
            // through `globals` and takes no env slot for it.
            //
            // Only a TOP-LEVEL one: `globals` is keyed by name, so a
            // block's own `let x` shadowing a global `var x` matched
            // this gate and kept its box — leaving the closure minted
            // above it reading the global. The scope stack is one deep
            // exactly where a declaration is the global.
            if ctx.is_main_fn && ctx.scope_stack.len() == 1 && ctx.globals.contains_key(&name) {
                continue;
            }
            // PROVISIONAL type: non-Copy, so the mint conservatively
            // wires the env's `__env_trace_*` twin (a Copy answer there
            // would store 0 and leave a cycle through this slot
            // unreachable to the collector — the one direction that
            // cannot be repaired after the fact). The declaration
            // patches it to what the init really produced.
            open_box(ctx, &name, Type::Any);
            ctx.forward_capture_boxes.insert(name, Vec::new());
            continue;
        };
        let Some(ty) = box_ty(ctx, &name, type_ann.as_ref(), init, &fn_name, mutable) else {
            continue;
        };
        crate::ssa_lower_stmt_let_decl_general::record_binding_flags(
            ctx,
            &name,
            type_ann.as_ref(),
            init,
        );
        open_box(ctx, &name, ty);
        ctx.hoisted_closure_lets.insert(name);
    }
}

/// One binding the walk decided needs its box up front. `fn_name` is
/// the lifted closure the init mints — `None` marks an ordinary
/// binding, whose type is not knowable yet.
struct Planned {
    name: String,
    type_ann: Option<String>,
    init: ExprId,
    fn_name: Option<String>,
    mutable: bool,
}

/// Walk the list in order, accumulating the names captured so far and
/// picking out each closure-initialized binding one of them already
/// named. `Stmt::Multi` is a transparent grouping sharing the
/// surrounding scope, so it flattens in; anything opening a scope of
/// its own does not — that block runs this pass itself.
fn collect<'s>(
    ctx: &LowerCtx,
    stmts: impl Iterator<Item = &'s Stmt>,
    seen_captures: &mut HashSet<String>,
    plan: &mut Vec<Planned>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                mutable,
                ..
            } => {
                let closure = match ctx.ast.get_expr(*init) {
                    Expr::Closure { fn_name, captures } => Some((fn_name.clone(), captures)),
                    _ => None,
                };
                let planned = seen_captures.contains(name);
                if planned {
                    plan.push(Planned {
                        name: name.clone(),
                        type_ann: type_ann.clone(),
                        init: *init,
                        fn_name: closure.as_ref().map(|(f, _)| f.clone()),
                        mutable: *mutable,
                    });
                }
                if let Some((_, captures)) = closure {
                    for c in captures {
                        seen_captures.insert(c.clone());
                    }
                } else {
                    // A closure minted DEEPER in the init (object-
                    // literal method, array element, `new` argument)
                    // forward-references the same way — collect its
                    // captures so a later binding gets its box (or,
                    // at top level, rides the data-global lane).
                    let mut nested: Vec<&str> = Vec::new();
                    crate::ast::nested_closure_captures::collect(ctx.ast, *init, &mut nested);
                    // 400-01 — a closure nested in THIS init capturing
                    // THIS binding (`const a: any = [function () { …
                    // a … }]`). The membership check above ran before
                    // the init's captures were seen, so a
                    // self-capture never planned and the mint later
                    // panicked ("closure capture not in scope"). The
                    // ordinary-binding arm (fn_name: None) is exactly
                    // right for it: the box goes up first, the mint
                    // captures the box, the declaration fills it. A
                    // BARE closure init's self-capture stays with
                    // `try_lower`'s inline case, untouched.
                    if !planned && nested.iter().any(|c| *c == name) {
                        plan.push(Planned {
                            name: name.clone(),
                            type_ann: type_ann.clone(),
                            init: *init,
                            fn_name: None,
                            mutable: *mutable,
                        });
                    }
                    for c in nested {
                        seen_captures.insert(c.to_string());
                    }
                }
            }
            Stmt::Multi(inner) => collect(ctx, inner.iter(), seen_captures, plan),
            // Every other statement position mints closures too — an
            // assignment, a call argument, a condition, a nested block
            // of this same region. §8.2.6 creates the block's lexical
            // bindings on entry, so one of those closures naming a
            // binding declared LATER in this list means THAT binding,
            // and the box has to be up before the mint runs.
            other => {
                let mut caps: Vec<&str> = Vec::new();
                crate::ast::stmt_closure_captures::collect_stmt(ctx.ast, other, &mut caps);
                for c in caps {
                    seen_captures.insert(c.to_string());
                }
            }
        }
    }
}

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mutable: bool,
) -> bool {
    let Expr::Closure { fn_name, captures } = ctx.ast.get_expr(init) else {
        return false;
    };
    let fn_name = fn_name.clone();
    let self_capture = captures.iter().any(|c| c == name);
    // The box may already be waiting — `hoist_forward_boxes` opened it
    // for this list because an earlier closure captured this name.
    if ctx.hoisted_closure_lets.remove(name) {
        let slot = ctx
            .locals
            .get(name)
            .expect("hoisted binding is registered")
            .slot;
        let init_val = ctx.lower_expr(init);
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(init_val, Operand::Value(slot), 0),
        );
        return true;
    }
    if !self_capture {
        return false;
    }
    let Some(ty) = box_ty(ctx, name, type_ann, init, &fn_name, mutable) else {
        return false;
    };
    crate::ssa_lower_stmt_let_decl_general::record_binding_flags(ctx, name, type_ann, init);
    let slot = open_box(ctx, name, ty);
    let init_val = ctx.lower_expr(init);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(init_val, Operand::Value(slot), 0),
    );
    true
}
