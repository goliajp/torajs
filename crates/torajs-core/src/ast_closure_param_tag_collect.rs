//! Snapshot/collection half of `ast_closure_param_tag` — carved out
//! when the alias-let axis pushed the main file past the 500-line
//! bar. Everything here ANSWERS a program-wide question ("which
//! decls have fn-typed params", "which lets hold closure-shaped
//! values") for the marking fixpoint the main pass runs; nothing
//! here mutates the tree.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

use crate::ast_closure_param_tag::{is_fnsig_ann, push_child_stmts};

/// Recursively snapshot FnDecls (top-level and nested) into the
/// param-index / signature maps.
pub(crate) fn collect_fn_decls(
    stmts: &[Stmt],
    fn_params: &mut HashMap<String, Vec<(usize, String)>>,
    fn_sigs: &mut HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    existing_forwarders: &mut HashSet<String>,
) {
    let mut stack: Vec<&Stmt> = stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        {
            if crate::ast::is_shell(name) {
                existing_forwarders.insert(name.clone());
            } else {
                let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
                let fnsig_params: Vec<(usize, String)> = params
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| is_fnsig_ann(&p.type_ann))
                    .map(|(i, p)| (i, p.name.clone()))
                    .collect();
                if !fnsig_params.is_empty() {
                    fn_params.insert(name.clone(), fnsig_params);
                }
                if !is_closure_shaped {
                    fn_sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
                }
            }
        }
        push_child_stmts(s, &mut stack);
    }
}

/// Snapshot, per FnDecl with an `__fn(`-annotated return type or none
/// at all, the ExprIds of every `return <e>` in its own body; the
/// unannotated ones also land in `unannotated`, whose returns the
/// marking round reads without the slot-read arm. A nested FnDecl
/// switches context — its returns attribute to itself, never to the
/// enclosing fn. A generator answers its iterator, never what it
/// returns, so it stays out.
///
/// An unannotated function returning a closure answers a closure (the
/// desugar infers its return from those returns later). Left out, a
/// call to it never read as closure-shaped, so `caller(mk(1))` kept
/// `caller`'s fn-typed parameter on the bare-pointer lane and jumped
/// into the closure cell's header (SIGBUS).
pub(crate) fn collect_fn_returns(
    stmts: &[Stmt],
    out: &mut HashMap<String, Vec<ExprId>>,
    unannotated: &mut HashSet<String>,
) {
    let mut stack: Vec<(&Stmt, Option<String>)> = stmts.iter().map(|s| (s, None)).collect();
    while let Some((s, cur)) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            return_type,
            body,
            is_generator,
            ..
        } = s
        {
            let ctx = if is_fnsig_ann(return_type) {
                Some(name.clone())
            } else if return_type.is_none() && !*is_generator {
                unannotated.insert(name.clone());
                Some(name.clone())
            } else {
                None
            };
            for b in body {
                stack.push((b, ctx.clone()));
            }
            continue;
        }
        if let Stmt::Return(Some(eid)) = s
            && let Some(f) = &cur
        {
            out.entry(f.clone()).or_default().push(*eid);
        }
        let mut kids: Vec<&Stmt> = Vec::new();
        push_child_stmts(s, &mut kids);
        for k in kids {
            stack.push((k, cur.clone()));
        }
    }
}

/// Closure-holding let bindings, program-wide by name: a closure
/// literal init, or a hoisted generator / async-generator EXPRESSION
/// (`let f = function* () {}` → `let f = Ident(__genexpr_N)`,
/// `hoist_gen_fn_exprs`) — that binding's slot re-reprs Closure
/// downstream, so an `__fn(`-annotated param receiving `f` must
/// retag; left unmarked, the bare-pointer lane `blr`'d the cell (the
/// eval-code `gen-func-expr-*-cntns-arguments-*` SIGBUS family). A
/// plain named-fn init (`let f = h`) stays out: that slot keeps the
/// raw fn_addr_let direct lane (see [`fn_alias_lets`] for how it
/// joins when a marked param actually receives it), unless a binding
/// axis gives the slot the cell ([`cell_slot_let`]).
pub(crate) fn closure_holding_lets(ast: &Ast) -> HashSet<String> {
    let census = crate::ast_collect_fn_closure_init::ident_census(ast);
    let mut closure_idents: HashSet<String> = HashSet::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl {
            name,
            init,
            mutable,
            type_ann,
            ..
        } = s
        {
            if cell_slot_let(ast, &census, *mutable, type_ann.as_deref(), name, *init) {
                closure_idents.insert(name.clone());
            }
            match ast.get_expr(*init) {
                Expr::Closure { .. } => {
                    closure_idents.insert(name.clone());
                }
                Expr::Ident(n) if ast.genexpr_names.contains_key(n) => {
                    closure_idents.insert(name.clone());
                }
                _ => {}
            }
        }
        push_child_stmts(s, &mut stack);
    }
    closure_idents
}

/// A binding whose slot the forwarder collector, which runs after this
/// pass, makes a closure cell on one of its binding axes
/// ([`crate::ast_collect_fn_closure_init::IdentCensus::wraps_let`]): a
/// mutable fn-typed slot always is one; an immutable or unannotated one
/// is when its initializer may name a function. Left out, a fn-typed
/// param receiving it kept the bare-pointer lane and `blr`'d the cell
/// (`const g: F = h; fs.push(g); call(g)`, EXIT 138).
fn cell_slot_let(
    ast: &Ast,
    census: &crate::ast_collect_fn_closure_init::IdentCensus,
    mutable: bool,
    type_ann: Option<&str>,
    name: &str,
    init: ExprId,
) -> bool {
    use crate::ast_collect_fn_closure_init::is_fn_slot_ann;
    if !census.wraps_let(mutable, type_ann, name) || !type_ann.is_none_or(is_fn_slot_ann) {
        return false;
    }
    (mutable && type_ann.is_some())
        || crate::ast_closure_param_tag_member::passthrough_leaves(ast, init)
            .iter()
            .any(|&l| matches!(ast.get_expr(l), Expr::Ident(_)))
}

/// `let x = <named-fn Ident>` aliases, program-wide by name:
/// x → (init ExprId, target fn). When a MARKED param receives `x`,
/// the alias's init wraps to `Closure { __forward_<target> }` so the
/// binding's slot re-reprs Closure alongside the retagged param —
/// otherwise a mixed call set (`apply(() => 41)` marks the param,
/// `apply(alias)` hands it a raw code pointer) blr's the raw address
/// through the env-first lane. Same scope-approximate grade as
/// [`closure_holding_lets`]; the fixpoint folds a wrapped alias back
/// into `closure_idents`, so its OTHER uses (another fn-typed param,
/// a return) mark consistently too.
pub(crate) fn fn_alias_lets(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
) -> HashMap<String, (ExprId, String)> {
    let mut out: HashMap<String, (ExprId, String)> = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Ident(n) = ast.get_expr(*init)
            && fn_sigs.contains_key(n)
        {
            out.insert(name.clone(), (*init, n.clone()));
        }
        push_child_stmts(s, &mut stack);
    }
    out
}

/// r549 — `let t = <Expr::Closure { fn_name }>` bindings: ident →
/// (lifted decl name, count of leading synthetic params). A
/// `const t = (f: () => n) => f()` arrow is CALLED by its binding
/// name, never by `__closure_N`, so the call-site rounds keyed on the
/// callee ident never reached the lifted decl's fn-typed params:
/// `t(() => 1)` handed a closure cell to a `__fn(` slot and the body
/// blr'd the cell's heap header (EXIT 138), while the same program
/// spelled `function t(...)` worked. The driver mirrors each aliased
/// decl's fn-typed params / returns under the ident (arg-indexed —
/// the lifted decl's `__env` / `__this` prefix is shifted off) and
/// folds the marks back onto the decl before the retag. Same
/// program-wide-by-name grade as [`closure_holding_lets`]: an ident
/// two functions both bind to an arrow (`const inner = …` in each)
/// names every such decl, the first-listed the one met last in the
/// walk, and the marks land on all of them — a call through the
/// ident cannot tell them apart, so they share one repr per slot.
pub(crate) type ClosureAliases = HashMap<String, Vec<(String, usize)>>;

pub(crate) fn closure_let_aliases(ast: &Ast) -> ClosureAliases {
    let mut prefix: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s {
            let n = params
                .iter()
                .take_while(|p| p.name == "__env" || p.name == "__this")
                .count();
            prefix.insert(name.clone(), n);
        }
        push_child_stmts(s, &mut stack);
    }
    let mut out: ClosureAliases = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && let Some(&n) = prefix.get(fn_name)
        {
            out.entry(name.clone())
                .or_default()
                .insert(0, (fn_name.clone(), n));
        }
        push_child_stmts(s, &mut stack);
    }
    out
}

/// Mirror each aliased decl's fn-typed params (arg-indexed) and
/// return sites under the binding ident — see [`closure_let_aliases`].
/// A real FnDecl of the same name keeps its own entry.
pub(crate) fn mirror_closure_aliases(
    aliases: &ClosureAliases,
    fn_params: &mut HashMap<String, Vec<(usize, String)>>,
    fn_returns: &mut HashMap<String, Vec<ExprId>>,
    unannotated: &mut HashSet<String>,
) {
    for (ident, decls) in aliases {
        // a real FnDecl of the same name keeps its own entry
        let (own_params, own_rets) = (
            fn_params.contains_key(ident),
            fn_returns.contains_key(ident),
        );
        for (fname, shift) in decls {
            if unannotated.contains(fname) {
                unannotated.insert(ident.clone());
            }
            if !own_params && let Some(fps) = fn_params.get(fname) {
                let shifted: Vec<(usize, String)> = fps
                    .iter()
                    .filter(|(i, _)| *i >= *shift)
                    .map(|(i, n)| (i - shift, n.clone()))
                    .collect();
                let slot = fn_params.entry(ident.clone()).or_default();
                for p in shifted {
                    if !slot.contains(&p) {
                        slot.push(p);
                    }
                }
            }
            if !own_rets && let Some(rets) = fn_returns.get(fname) {
                let rets = rets.clone();
                fn_returns.entry(ident.clone()).or_default().extend(rets);
            }
        }
    }
}

/// The usage-axis seeds of the marking fixpoint: params whose callers
/// hand closure repr however the call sites read. Chunk 631: a
/// replace callback's (see the axes sibling); 551-01: a builtin HOF
/// callback's element-position ones (see the hof sibling); r549: a
/// closure-let alias's ([`seed_closure_alias_marks`]); a parameter
/// called with an `any` argument for an object position (the axes
/// sibling); a parameter the function stores into the heap (the
/// store-sink axis).
pub(crate) fn usage_seed_marks(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    aliases: &ClosureAliases,
) -> HashSet<(String, usize)> {
    let mut out = crate::ast_closure_param_tag_axes::replace_cb_param_seeds(ast, fn_params);
    out.extend(crate::ast_closure_param_tag_hof::hof_elem_cb_param_seeds(
        ast, fn_params,
    ));
    out.extend(crate::ast_closure_param_tag_axes::any_obj_arg_seeds(
        ast, fn_params,
    ));
    out.extend(crate::ast_closure_param_tag_axes::store_sink_param_seeds(
        ast, fn_params,
    ));
    out.extend(seed_closure_alias_marks(aliases, fn_params));
    out
}

/// Seed marks for every fn-typed param of an aliased decl (by the
/// binding ident, arg-indexed). The binding is a Closure-typed slot,
/// and the closure call lane hands EVERY fn value to it in closure
/// repr — a named fn is forwarder-wrapped at the call site
/// (`fnprops_bind_cell` on a `__forward_<g>` cell) — so a `__fn(`
/// param there would `blr` the cell's header no matter what the
/// call sites look like (`t(g)` alone, EXIT 138). The decl's params
/// therefore take the closure repr unconditionally; the call-site
/// rounds still run for the return lane and the transitive marks.
fn seed_closure_alias_marks(
    aliases: &ClosureAliases,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for ident in aliases.keys() {
        if let Some(fps) = fn_params.get(ident) {
            out.extend(fps.iter().map(|(i, _)| (ident.clone(), *i)));
        }
    }
    out
}

/// Fold marks that landed on a binding ident back onto the lifted
/// decl (param index shifted back past the synthetic prefix) so the
/// in-place retag finds the FnDecl. The ident entries stay: the
/// wrap rounds match call sites by the callee ident.
pub(crate) fn fold_closure_alias_marks(
    aliases: &ClosureAliases,
    marked: &mut HashSet<(String, usize)>,
    ret_marked: &mut HashSet<String>,
) {
    for (ident, decls) in aliases {
        for (fname, shift) in decls {
            let folded: Vec<(String, usize)> = marked
                .iter()
                .filter(|(f, _)| f == ident)
                .map(|(_, i)| (fname.clone(), i + shift))
                .collect();
            marked.extend(folded);
            if ret_marked.contains(ident) {
                ret_marked.insert(fname.clone());
            }
        }
    }
}
