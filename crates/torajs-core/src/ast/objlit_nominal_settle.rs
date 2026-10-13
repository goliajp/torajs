//! Apply phase of [`super::objlit_nominal`] — what its collect loop
//! produced, landed on the AST. Split out at the 500-line file limit;
//! the receiver-shape decisions themselves stay documented at their
//! mint sites in the host.

use std::collections::{HashMap, HashSet};

use super::objlit_nominal::MethodPatch;
use super::{Expr, ExprId, Stmt};

/// What the collect loop above produced, handed to the apply phase in
/// one piece.
pub(super) struct Settle<'a> {
    pub(super) stmts: &'a mut Vec<Stmt>,
    pub(super) exprs: &'a mut Vec<Expr>,
    /// Anylane members promoted to the `__this: any` receiver-first
    /// shape — `(face ExprId, lifted fn name)`.
    pub(super) any_patches: Vec<(ExprId, String)>,
    /// Nominal members: the `__this: __ObjLit_n` receiver patches.
    pub(super) patches: Vec<MethodPatch>,
    /// The accessor subset of `patches` whose body never says `this`
    /// — marked receiver-first once the nominal patches land.
    pub(super) recvless_accessors: Vec<String>,
    /// The synthetic `__ObjLit_n` aliases, `__mth_placeholder` fields
    /// still parked in them.
    pub(super) type_decls: Vec<Stmt>,
    pub(super) fn_sigs: &'a mut HashMap<String, String>,
    pub(super) fnexpr_recv_fns: &'a mut std::collections::HashSet<String>,
    pub(super) sloppy: super::sloppy_this_prologue::SloppyGoal<'a>,
    pub(super) spans: &'a mut Vec<crate::lexer::Span>,
}

/// Apply phase — promote the anylane faces, then land the nominal
/// patches and publish the aliases they name.
pub(super) fn settle_collected(s: Settle<'_>) {
    if !s.any_patches.is_empty() {
        super::fnexpr_this::promote_recv_any(
            s.stmts,
            s.exprs,
            &s.any_patches,
            s.fnexpr_recv_fns,
            s.sloppy,
            s.spans,
        );
    }
    if s.patches.is_empty() {
        return;
    }
    let mut type_decls = s.type_decls;
    super::objlit_nominal::apply_patches(
        s.stmts,
        s.exprs,
        &s.patches,
        &mut type_decls,
        s.fn_sigs,
        s.fnexpr_recv_fns,
    );
    // Rotation 461 — an accessor takes the receiver whether or not it
    // says `this`, and a this-FREE one never READS the slot it was
    // given. Which lane the literal ends up on is not decidable here
    // (the checker's Any answer at a call argument is the leak every
    // any-lane leg keeps missing), so mark those faces receiver-first:
    // the dynobj accessor install then picks BOXED|RECV and its argv
    // lines up with the declared params, instead of hitting
    // `guard_anylane_recv_face`.
    //
    // The `__this` ANNOTATION deliberately stays nominal. It is not
    // dead metadata — the width analysis reads it to join a member fn
    // to its literal, and re-annotating it `any` split the caller's
    // slot projection from the body's own narrowing (measured: caller
    // passed the setter value in V0, callee read X3). The receiver
    // that arrives on the dynobj lane is a box the body never
    // dereferences, so the lie costs nothing there.
    s.fnexpr_recv_fns.extend(s.recvless_accessors);
    s.stmts.extend(type_decls);
}

/// The object-literal accessor slots whose closure the arguments pass
/// put on the argv face. Its native entry carries the raw argv pointer,
/// so only the boxed dual entry may call it; the boxed entry hands a
/// receiver-first closure its `this` (every closure with a `__this`
/// param is marked so) and leaves a this-free one without.
#[derive(Debug, Clone, Default)]
pub struct ObjlitArgvSlots {
    /// (the literal's field names in order, the slot, the closure's fn
    /// name). A receiver is known only by its layout, which literals
    /// with the same field names can share, so a lane matches by names.
    pub rows: Vec<(Vec<String>, String, String)>,
    /// The bindings that can only ever hold one accessor literal: the
    /// program declares the name once, `const`, initialized by that
    /// literal, and never as a param or an assignment target. A lane
    /// reading through one knows which closure sits in the slot.
    pub bound: HashMap<String, ExprId>,
}

/// Fill `ast.objlit_argv_slots` (see [`ObjlitArgvSlots`]).
pub fn collect_argv_slots(ast: &mut super::Ast) {
    if ast.closure_argv_fns.is_empty() {
        return;
    }
    let mut rows: Vec<(Vec<String>, String, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::ObjectLit { fields } = e else {
            continue;
        };
        for (key, feid) in fields.iter().filter(|(k, _)| is_accessor(k)) {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid)
                && ast.closure_argv_fns.contains(fn_name)
            {
                let names: Vec<String> = fields
                    .iter()
                    .filter_map(|(k, _)| k.as_str().map(str::to_string))
                    .collect();
                let row = (names, key.lossy().to_string(), fn_name.clone());
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
        }
    }
    if rows.is_empty() {
        return;
    }
    let bound = bound_literals(ast);
    ast.objlit_argv_slots = ObjlitArgvSlots { rows, bound };
}

fn is_accessor(k: &crate::ast::PropKey) -> bool {
    k.as_str()
        .is_some_and(|k| k.starts_with("__getter_") || k.starts_with("__setter_"))
}

/// [`ObjlitArgvSlots::bound`]: a name declared once, `const`, by an
/// accessor literal, and bound or written nowhere else.
fn bound_literals(ast: &super::Ast) -> HashMap<String, ExprId> {
    // name → (declarations, the literal of the last)
    let mut decls: HashMap<String, (usize, Option<ExprId>)> = HashMap::new();
    let mut out: HashSet<String> = HashSet::new();
    super::ns_alias::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            init,
            mutable,
            ..
        } = s
        {
            let lit = matches!(ast.get_expr(*init), Expr::ObjectLit { fields }
                if fields.iter().any(|(k, _)| is_accessor(k)));
            let d = decls.entry(name.clone()).or_insert((0, None));
            d.0 += 1;
            d.1 = lit.then_some(*init);
            if *mutable {
                out.insert(name.clone());
            }
        }
        other_binders(s, &mut out);
    });
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && let Expr::Ident(n) = ast.get_expr(*target)
        {
            out.insert(n.clone());
        }
    }
    decls
        .into_iter()
        .filter(|(n, (count, _))| *count == 1 && !out.contains(n))
        .filter_map(|(n, (_, lit))| lit.map(|l| (n, l)))
        .collect()
}

/// The names `s` binds other than through a `let` / `const`.
fn other_binders(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::FnDecl { name, params, .. } => {
            out.insert(name.clone());
            out.extend(params.iter().map(|p| p.name.clone()));
        }
        Stmt::ForOf { var_name: n, .. }
        | Stmt::ForOfSplitIter { var_name: n, .. }
        | Stmt::UsingDecl { name: n, .. }
        | Stmt::YieldInto { var: n, .. }
        | Stmt::Try {
            catch_param: Some(n),
            ..
        }
        | Stmt::ClassDecl { name: n, .. } => {
            out.insert(n.clone());
        }
        Stmt::ImportDecl {
            default,
            namespace,
            named,
            ..
        } => {
            out.extend(default.iter().chain(namespace).cloned());
            out.extend(
                named
                    .iter()
                    .map(|(n, a)| a.clone().unwrap_or_else(|| n.clone())),
            );
        }
        // The walk does not enter an export; its binding stays unknown.
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => {
            if let Stmt::LetDecl { name, .. } = &**inner {
                out.insert(name.clone());
            }
        }
        _ => {}
    }
}
