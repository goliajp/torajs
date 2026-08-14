//! The receiver-safe USE SHAPES of a knife-2 promotion candidate,
//! collected in one place.
//!
//! A `this`-using function expression only gets its receiver promoted
//! when every use of its binding program-wide is a shape the promoted
//! ABI survives — the promoted body carries an extra leading `__this`
//! parameter, so a receiver-unaware call path would shift every
//! argument. This module answers "which nodes are those", and
//! [`super::fnexpr_this_routed`] answers "does this binding qualify".
//!
//! There are two kinds of proof behind the list, and keeping them
//! apart is what makes adding a shape a decidable question rather than
//! a hunch:
//!
//! 1. **It never calls the binding.** A member's object, the right of
//!    `instanceof`, an equality operand, the target of
//!    `Object.defineProperty` — these consume the cell as a value and
//!    enter no call lane at all.
//! 2. **The value crosses into the any lane and stays there.** An
//!    explicitly-`any` parameter slot, and a `return` across an
//!    any-typed boundary. The cell escapes, so the proof has to cover
//!    every downstream path — which holds because every any-lane call
//!    path shifts argv on `FLAG_CLOSURE_RECV_FIRST`, while a typed
//!    indirect call does not.
//!
//! A direct call is the one member of neither kind: it is receiver-safe
//! because the CALL SITE is rewritten to seed `undefined` into the
//! promoted slot (§10.2.1.2), not because of anything about the value.
//!
//! Split out of `fnexpr_this_routed.rs` in rotation 397 under the
//! function-size rule: `promote_variable_routed` had grown to 187 lines
//! as the confluence of nine shapes, and every shape added a few more.
//! The move is what the file-size debt table asked the next
//! shape-adder to do first.

use super::fnexpr_this_args::{
    any_param_arg_idents, construct_channel_arg_idents, define_property_target_idents,
    eq_operand_idents, instanceof_name_idents,
};
use super::fnexpr_this_names::peel_as;
use super::fnexpr_this_recvs::{collect_any_binding_names, collect_arraylit_binding_names};
use super::{Expr, ExprId, Stmt};

/// Every receiver-safe position in the program, by kind.
pub(super) struct UseShapes {
    /// Direct-call callee (`h(args)`) — receiver-safe because
    /// `ssa_lower_call_closure_local` seeds `undefined` into the
    /// promoted `__this` slot at the site.
    pub(super) callee: std::collections::HashSet<ExprId>,
    /// A member's OBJECT, `.call` / `.apply` / `.bind` excepted:
    /// those read the binding in order to invoke it, so they would
    /// see the shifted-args ABI (they ride the `call_faces` replay bar
    /// instead). `Ctor.prototype` was the first consumer; since
    /// rotation 394 the whole shape admits, which is how a static
    /// method is spelled once a class lowers to the ES5 constructor
    /// pattern — `Ctor.s()` calls what the property HOLDS.
    pub(super) member_obj: std::collections::HashSet<ExprId>,
    /// An argument to a construct-channel builtin, and the
    /// `NewDynamic` callee.
    pub(super) construct_arg: std::collections::HashSet<ExprId>,
    /// An equality operand.
    pub(super) eq_operand: std::collections::HashSet<ExprId>,
    /// An argument in an explicitly-`any` (or proven-safe generic)
    /// parameter slot of a program-local FnDecl.
    pub(super) any_arg: std::collections::HashSet<ExprId>,
    /// The bare name on the right of `instanceof`.
    pub(super) instanceof_name: std::collections::HashSet<ExprId>,
    /// `return <bare name>` out of a function whose return type is
    /// inferred or spelled exactly `any`. Admits only together with
    /// [`Self::any_ann_names`] — see [`any_boundary_return_idents`].
    pub(super) any_return: std::collections::HashSet<ExprId>,
    /// The target argument of `Object.defineProperty` /
    /// `defineProperties`.
    pub(super) define_target: std::collections::HashSet<ExprId>,
    /// Names a `LetDecl` annotates exactly `any` — the binding half of
    /// the return-shape proof.
    pub(super) any_ann_names: std::collections::HashSet<String>,
    /// The `<name> as any` callback slot of an any-lane-certain array
    /// HOF call (401-03) — see [`hof_any_cb_arg_idents`].
    pub(super) hof_cb_arg: std::collections::HashSet<ExprId>,
    /// An ELEMENT of an array literal initializing an exactly-`any`
    /// binding (397-01) — see [`any_arraylit_elem_idents`].
    pub(super) any_arraylit_elem: std::collections::HashSet<ExprId>,
    /// The init site of a PROVEN-SAFE alias declaration (397-02) —
    /// see [`super::fnexpr_this_alias`]. Filled in a second pass of
    /// `collect`, since the fixpoint consults the base shapes.
    pub(super) safe_alias_init: std::collections::HashSet<ExprId>,
    /// An argument of `.call` on an INLINE function expression —
    /// see [`inline_fnexpr_call_arg_idents`].
    pub(super) inline_call_arg: std::collections::HashSet<ExprId>,
}

impl UseShapes {
    pub(super) fn collect(stmts: &[Stmt], exprs: &[Expr]) -> Self {
        let mut any_ann_names = std::collections::HashSet::new();
        collect_any_ann_decl_names(stmts, &mut any_ann_names);
        let mut shapes = Self {
            callee: exprs
                .iter()
                .filter_map(|e| match e {
                    Expr::Call { callee, .. }
                        if matches!(&exprs[callee.0 as usize], Expr::Ident(_)) =>
                    {
                        Some(*callee)
                    }
                    _ => None,
                })
                .collect(),
            member_obj: exprs
                .iter()
                .filter_map(|e| match e {
                    // Peeling the `as` suffix changes nothing about the
                    // `.call` / `.apply` / `.bind` exclusion — that
                    // reads the member NAME, which the wrapper does not
                    // touch.
                    Expr::Member { obj, name }
                        if !matches!(name.as_str(), "call" | "apply" | "bind") =>
                    {
                        let obj = peel_as(exprs, *obj);
                        matches!(&exprs[obj.0 as usize], Expr::Ident(_)).then_some(obj)
                    }
                    _ => None,
                })
                .collect(),
            construct_arg: construct_channel_arg_idents(exprs),
            eq_operand: eq_operand_idents(exprs),
            any_arg: any_param_arg_idents(stmts, exprs),
            instanceof_name: instanceof_name_idents(exprs),
            any_return: any_boundary_return_idents(stmts, exprs),
            define_target: define_property_target_idents(exprs),
            any_ann_names,
            hof_cb_arg: hof_any_cb_arg_idents(stmts, exprs),
            any_arraylit_elem: any_arraylit_elem_idents(stmts, exprs),
            safe_alias_init: std::collections::HashSet::new(),
            inline_call_arg: inline_fnexpr_call_arg_idents(exprs),
        };
        shapes.safe_alias_init =
            super::fnexpr_this_alias::safe_alias_init_sites(stmts, exprs, &shapes);
        shapes
    }

    /// Is this use of `name` receiver-safe?
    pub(super) fn admits(&self, e: ExprId, name: &str) -> bool {
        self.callee.contains(&e)
            || self.member_obj.contains(&e)
            || self.construct_arg.contains(&e)
            || self.eq_operand.contains(&e)
            || self.any_arg.contains(&e)
            || self.instanceof_name.contains(&e)
            || self.define_target.contains(&e)
            || self.hof_cb_arg.contains(&e)
            || self.any_arraylit_elem.contains(&e)
            || self.safe_alias_init.contains(&e)
            || self.inline_call_arg.contains(&e)
            || (self.any_return.contains(&e) && self.any_ann_names.contains(name))
    }
}

/// An argument of `.call` on an INLINE function expression — the
/// static-init wrapper the capturing-class lane mints:
/// `(function () { … }).call(K)` (394-05).
///
/// Never-calls proof family (a member's object, the right of
/// `instanceof`): the thing being invoked is the inline literal, and
/// the binding rides along as DATA — the wrapper's receiver, or a
/// plain argument. Inside the wrapper it arrives as an untyped `this`
/// or parameter, both of which live in the any world, whose every
/// call path shifts argv on FLAG_CLOSURE_RECV_FIRST.
///
/// The `.call` exclusion on [`UseShapes::member_obj`] is about the
/// OTHER position — the binding as the `.call` target, where the
/// promoted cell itself is what gets invoked. An inline-literal
/// callee cannot be the promoted binding — a binding is read through
/// its NAME, and both matched shapes are literal values at the site
/// (`lift_arrow_fns` has replaced the still-inline fn expressions
/// with `Expr::Closure` by the time this collector runs, so both
/// spellings appear). Only the spelled `.call` — the wrapper never
/// mints `.apply` / `.bind`, and widening to them would need their
/// own reading.
fn inline_fnexpr_call_arg_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
            continue;
        };
        if name != "call" {
            continue;
        }
        let obj = peel_as(exprs, *obj);
        if !matches!(
            &exprs[obj.0 as usize],
            Expr::ArrowFn { .. } | Expr::Closure { .. }
        ) {
            continue;
        }
        for a in args {
            let inner = peel_as(exprs, *a);
            if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                out.insert(inner);
            }
        }
    }
    out
}

/// 397-01 — an element of an array literal that initializes an
/// exactly-`any` binding: `const arr: any = [g]`.
///
/// Escaping proof family (the explicit-`any` argument / any-boundary
/// return shapes): the binding is `any`, so the whole array lives in
/// the any world and every read of an element stays there — an
/// `arr[0](7)` rides the any-index call lane, whose closure leg
/// dispatches through `invoke_with_this` (the 399-05 fix), and a
/// detached read's plain call seeds `undefined`; both shift argv on
/// FLAG_CLOSURE_RECV_FIRST. The annotation must be spelled on the
/// binding itself — an inferred array type rides the typed lanes,
/// whose element calls do not shift.
///
/// Bare Ident elements and their `as` shells both admit (the shell
/// changes the checker's view, never the value, and the binding's
/// own `any` is what decides the lane).
fn any_arraylit_elem_idents(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    fn walk(stmts: &[Stmt], exprs: &[Expr], out: &mut std::collections::HashSet<ExprId>) {
        for s in stmts {
            if let Stmt::LetDecl { type_ann, init, .. } = s
                && type_ann.as_deref() == Some("any")
            {
                let init = peel_as(exprs, *init);
                if let Expr::Array(elems) = &exprs[init.0 as usize] {
                    for e in elems {
                        let inner = peel_as(exprs, *e);
                        if matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
                            out.insert(inner);
                        }
                    }
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| walk(inner, exprs, out));
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(stmts, exprs, &mut out);
    out
}

/// The `<name> as any` callback slot of an array HOF call whose
/// receiver is any-lane-certain (401-03).
///
/// This is the escaping proof family (the explicit-`any` argument /
/// any-boundary return shapes): the cell crosses into the any lane
/// and stays there, because every any-lane call path shifts argv on
/// FLAG_CLOSURE_RECV_FIRST. Three links, each carried by a syntactic
/// certainty:
///
/// 1. the `as any` shell makes the checker see `Any` in the slot, so
///    the call routes through the any-lane HOF wedge
///    (`check_type_of_call_arr_hof_any_cb`) on a typed receiver and
///    the any-method route on an `any` one — never the typed inline
///    loop, whose argv does not shift;
/// 2. the receiver is an inline array literal or a binding the
///    knife-4 certainty collectors prove only ever holds one
///    (`collect_arraylit_binding_names` / an `: any` binding) — a
///    user object with its own `map` method decides its own call
///    protocol and stays out;
/// 3. the method set mirrors the wedge's — the ones whose runtime
///    kernels (`__torajs_arr_any_map` and friends, `find_loop`'s
///    backward modes, the flatMap walk) seed argv[0] off the header
///    flag.
///
/// The BARE spelling (`xs.map(cb, t)`) is deliberately not here: it
/// types at the closure's signature and rides the typed knife-4
/// lanes, which thread the receiver through `sig_skip` instead.
fn hof_any_cb_arg_idents(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let any_recvs = collect_any_binding_names(stmts, exprs);
    let arraylit_recvs = collect_arraylit_binding_names(stmts, exprs);
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
            continue;
        };
        if !matches!(
            name.as_str(),
            "map"
                | "filter"
                | "forEach"
                | "every"
                | "some"
                | "find"
                | "findIndex"
                | "findLast"
                | "findLastIndex"
                | "flatMap"
                | "reduce"
                | "reduceRight"
        ) || args.is_empty()
        {
            continue;
        }
        // Only the As-wrapped spelling — see the bare-spelling note.
        let Expr::As { expr, ty_ann } = &exprs[args[0].0 as usize] else {
            continue;
        };
        if ty_ann != "any" {
            continue;
        }
        let inner = peel_as(exprs, *expr);
        if !matches!(&exprs[inner.0 as usize], Expr::Ident(_)) {
            continue;
        }
        let obj = peel_as(exprs, *obj);
        let recv_ok = matches!(&exprs[obj.0 as usize], Expr::Array(_))
            || matches!(&exprs[obj.0 as usize], Expr::Ident(n)
                if any_recvs.contains(n) || arraylit_recvs.contains(n));
        if recv_ok {
            out.insert(inner);
        }
    }
    out
}

/// `return <bare name>` out of a function whose return type is
/// inferred or spelled exactly `any`.
///
/// Returning the name never CALLS the binding — the same one-liner that
/// admits a member's object and the right of `instanceof`. What makes
/// this one different is that the cell ESCAPES, so the proof it needs
/// is the explicit-`any` argument shape's rather than theirs: the value
/// has to cross into the any lane and STAY there, because every
/// any-lane call path honors the receiver channel (`__torajs_any_call`
/// / `invoke_with_this` / the NewDynamic kernel all shift argv on
/// FLAG_CLOSURE_RECV_FIRST) while a typed indirect call does not.
///
/// Two things have to hold for that crossing, and this classifier only
/// checks the second — the caller pairs it with the binding's own `any`
/// annotation, which is what makes the RETURNED expression an any-lane
/// cell in the first place:
///
/// 1. the binding is annotated `any`, and
/// 2. the boundary does not re-type it — an absent return annotation
///    infers the `any` straight through, and an explicit `any` says it
///    outright. A concrete signature (`function take(f: any): (a:
///    number) => string`) is what this rejects: it hands the caller a
///    typed callee, whose call path never reads the flag.
///
/// The excluded halves stay loud, which is where they already are:
/// every program that returns a `this`-using binding is rejected today,
/// so nothing that answers correctly can be pulled in.
///
/// Bare Ident only, like the `instanceof` shape — `return C as any` is
/// a different node and is not measured here.
fn any_boundary_return_idents(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    fn walk(
        stmts: &[Stmt],
        exprs: &[Expr],
        admits: bool,
        out: &mut std::collections::HashSet<ExprId>,
    ) {
        for s in stmts {
            if let Stmt::Return(Some(eid)) = s
                && admits
                && matches!(&exprs[eid.0 as usize], Expr::Ident(_))
            {
                out.insert(*eid);
            }
            // A `return` belongs to the nearest enclosing function, so
            // the FnDecl arm re-derives `admits` while every other
            // compound form carries it through. Top level starts false:
            // a `return` cannot appear there, and guessing in the
            // admitting direction is the unsafe one.
            match s {
                Stmt::FnDecl {
                    return_type, body, ..
                } => walk(
                    body,
                    exprs,
                    return_type.as_deref().is_none_or(|a| a == "any"),
                    out,
                ),
                _ => super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                    walk(inner, exprs, admits, out)
                }),
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    walk(stmts, exprs, false, &mut out);
    out
}

/// Every name a `LetDecl` annotates exactly `any` — the binding half of
/// the return-shape proof above. A name declared twice lands here off
/// either decl, and the `decls.len() != 1` guard is what turns that
/// away.
fn collect_any_ann_decl_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        if let Stmt::LetDecl { name, type_ann, .. } = s
            && type_ann.as_deref() == Some("any")
        {
            out.insert(name.clone());
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_any_ann_decl_names(inner, out)
        });
    }
}
