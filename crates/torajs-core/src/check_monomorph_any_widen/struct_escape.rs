//! The all-`any` twin of a struct-parameter function behind a value
//! escape, and the relay shell that reaches it.
//!
//! `function f(p: P) { return p.x }` reached through `any` — `(f as
//! any)(v)`, `const g: any = f; g(v)` — enters its relay shell's boxed
//! adapter, which unboxes `v` for the typed body: a struct slot takes
//! the cell's pointer and the body reads `x` at the layout's offset.
//! That is a read of `v.x` only when `v` IS a `P` (or a class
//! extending it field for field). An array, a string, a Proxy, a
//! function, a `3`, `null` — anything else the any world hands over —
//! has no such slot, and the body read garbage: a pointer printed as a
//! number, a silent exit. A generic target takes a shell twin of its
//! own (`generic_shell`).
//!
//! §13.3.2.1 makes `p.x` a [[Get]] on the value `p` holds at the read,
//! whatever it is, and §10.2.1.3 binds the argument as it is. The
//! reading that honours both for an argument of unknown shape is the
//! any lane, so the adapter asks `__torajs_anyv_struct_fits` per struct
//! slot (`__torajs_anyv_arr_fits` per array-of-struct slot, over every
//! element) and, when a slot does not fit, hands the whole call to a twin
//! of the function with every parameter `any` — the policy the
//! direct-call wedge takes for an `any` argument against a heap-typed
//! parameter (`check_type_of_call/any_widen.rs`). The typed body is
//! untouched and still serves every fitting call.
//!
//! Two decls per shell. `<f>$$anyws` is the function cloned with every
//! non-rest parameter `any` and its return `any`: the body re-types
//! under `any`, and a declared `: number` would otherwise cast the
//! `undefined` an array answers for `x` into a number. `<shell>$$anyws`
//! is a relay shell over it on the same `__env` face, so the mono
//! adapter calls the twin's adapter with its own env, argv and argc
//! (`ssa_lower_boxed_entry::twin_adapter_of`). Like the 551-03 clones
//! the shell twin has no construction site: it is checked with the
//! original shell's site and reads the original's env layout
//! (`widen_clone_origin`).
//!
//! A `string` or array parameter takes the same twin: neither slot
//! holds `null`, `undefined` or another kind of value, so the adapter
//! tests it (`__torajs_anyv_str_fits` / `__torajs_anyv_arr_cell`) and
//! hands such an argument to the twin as passed. So does a `number` /
//! `boolean` parameter handed a value of another kind, which the
//! typed face would convert; that twin is cut only behind a shell
//! whose value may be called through `any` ([`target`]).
//!
//! A lifted closure needs no shell and takes one clone ([`closure`]).
//! A generator factory (async or not) takes the twin the desugar cut
//! from its source (`ast::generator_any_twin`) instead of a clone. An
//! async function's clone reaches its machine's generator through a
//! direct call with `any` arguments, which takes that generator's
//! twin. Out of reach, and left on the coercion kernel: a target with
//! an argv channel (`__torajs_argv`, new.target), a generic without an
//! object parameter, and a receiver channel (`__this`) except at the callback of an iteration
//! handing `any` elements (`escaped_cb`), where the receiver rides
//! argv[0] on both faces. Class methods take `__cmany_` / `__smany_`;
//! a class factory reached through the construct channel takes a clone
//! of its own ([`factory`]).

use std::collections::{HashMap, VecDeque};

mod boxed_reach;
pub(super) mod closure;
pub(crate) mod escaped_cb;
mod factory;
mod generic_shell;
mod named_cb;
mod pair;
mod target;
mod typed_sites;
use crate::ast::{Ast, Expr, ExprId, Param, ShellFace, Stmt};
use crate::check::Checker;
use crate::check_monomorph::WorkItem;
use crate::ssa_lower_generics_monomorph::Generics;
pub(crate) use boxed_reach::refuse_untwinned;
use pair::mint_pair;
pub(crate) use target::twin_clone_params;
pub(super) use target::widenable_target;

const SUFFIX: &str = "$$anyws";

/// The twin's name for `name` — a target, or its shell.
pub(crate) fn twin_name(name: &str) -> String {
    format!("{name}{SUFFIX}")
}

/// Is `name` a twin this lane minted?
pub(crate) fn is_twin(name: &str) -> bool {
    name.ends_with(SUFFIX)
}

/// The name `twin` is the twin of, when it is one.
pub(crate) fn twin_original(twin: &str) -> Option<&str> {
    twin.strip_suffix(SUFFIX)
}

/// Mint the twin pair behind every plain-face shell whose target
/// declares a struct parameter. Runs once, ahead of the widen
/// fixpoint: the twins' checks seed that loop like any clone's.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
) {
    let shells: Vec<(String, String)> = owned_ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if !name.contains("$$") => name
                .strip_prefix("__forward_")
                .map(|t| (name.clone(), t.to_string())),
            _ => None,
        })
        .collect();
    let loose_shells = closure::loose_values(c, owned_ast, "__forward_");
    for (shell, target) in shells {
        if c.generic_type_params.contains_key(&target) {
            let tables = (worklist as &mut _, call_retargets as &mut _);
            generic_shell::run(c, owned_ast, (generics, cache), tables, &shell, &target);
            continue;
        }
        let Some(found) = widenable_target(c, owned_ast, &target, false) else {
            continue;
        };
        if found.needs_escape && !loose_shells.contains(&shell) {
            continue;
        }
        // a generator's twin is a class of its own, and so is the machine
        // an async body's twin reaches: cut only for a shell whose value
        // may be called other than through its typed face
        if (owned_ast.generator_factory_classes.contains_key(&target)
            || owned_ast.async_fns.contains(&target))
            && !loose_shells.contains(&shell)
        {
            continue;
        }
        mint_pair(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            &shell,
            &target,
            &found.params,
        );
    }
    factory::run(
        c,
        owned_ast,
        generics,
        cache,
        worklist,
        call_retargets,
        mono_decls,
    );
}

fn construction_site(ast: &Ast, shell: &str) -> Option<ExprId> {
    ast.exprs
        .iter()
        .position(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == shell))
        .map(|i| ExprId(i as u32))
}

/// `<shell>$$anyws(__env, …user: any) { return <target twin>(…) }`,
/// registered the way a lifted clone is (551-03) and checked at the
/// original shell's construction site. A body that does not check
/// leaves the shell without a twin: the mono adapter then keeps the
/// coercion kernel for that function.
fn mint_shell_twin(
    c: &mut Checker,
    ast: &mut Ast,
    shell: &str,
    target_params: &[Param],
    target_twin: &str,
    site: ExprId,
) {
    let twin = twin_name(shell);
    let span = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, span, .. } if name == shell => Some(*span),
        _ => None,
    });
    let Some(span) = span else {
        return;
    };
    // the target twin's own face: every slot `any`, a rest slot as the
    // target spelled it (the shell forwards it as a spread of the same
    // element type)
    let user: Vec<Param> = target_params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: if p.is_rest {
                p.type_ann.clone()
            } else {
                Some("any".to_string())
            },
            default: p.default,
            is_rest: p.is_rest,
        })
        .collect();
    let ret = Some("any".to_string());
    let Ok(face) = crate::check_fn_type::build_fn_type_full(
        &twin,
        &user,
        &ret,
        &c.aliases,
        &[],
        &c.generic_alias_decls,
    ) else {
        return;
    };
    let decl = crate::ast::synth_forwarder_decl(
        ast,
        &twin,
        target_twin,
        &user,
        ret,
        span,
        ShellFace::of(shell),
    );
    // the pass that packs a call's rest tail (`ast::apply_rest_args`)
    // ran at desugar time; its single-spread form, a fresh array over
    // the source (§10.4.2 CreateArrayFromList), is spelled here
    if let Stmt::FnDecl { body, .. } = &decl
        && let [Stmt::Return(Some(call))] = body.as_slice()
        && let Expr::Call { args, .. } = ast.get_expr(*call).clone()
    {
        for a in args {
            let Expr::Spread { expr: src } = *ast.get_expr(a) else {
                continue;
            };
            let array_id = ast.add_expr(Expr::Ident("Array".into()));
            let from_id = ast.add_expr(Expr::Member {
                obj: array_id,
                name: "from".into(),
            });
            ast.exprs[a.0 as usize] = Expr::Call {
                callee: from_id,
                args: vec![src],
            };
        }
    }
    c.globals.insert(twin.clone(), face);
    c.closure_fn_names.insert(twin.clone());
    if let Some(d) = c.fn_defaults.get(shell).cloned() {
        c.fn_defaults.insert(twin.clone(), d);
    }
    crate::check_monomorph_closures::mirror_name_keyed_tables(ast, shell, &twin);
    c.widen_clone_origin.insert(twin.clone(), shell.to_string());
    ast.stmts.push(decl);
    let saved_errors = c.errors.len();
    let checked = crate::check_type_of_fn::check_closure_body(c, ast, site, &twin, Vec::new());
    if checked.is_err() || c.errors.len() > saved_errors {
        c.errors.truncate(saved_errors);
        ast.stmts
            .retain(|s| !matches!(s, Stmt::FnDecl { name, .. } if *name == twin));
        c.globals.remove(&twin);
        c.closure_fn_names.remove(&twin);
        c.fn_defaults.remove(&twin);
        c.widen_clone_origin.remove(&twin);
    }
}
