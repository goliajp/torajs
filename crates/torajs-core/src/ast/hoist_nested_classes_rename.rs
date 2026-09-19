//! α-rename walker: rewrite every reference to `old` inside a
//! statement subtree to `new` — `Ident`, `New { class_name }` and
//! sibling `extends` clauses. The
//! walk mirrors the free-vars traversal's coverage but mutates the
//! arena in place; ArrowFn bodies are taken out and re-seated so the
//! arena borrow never overlaps the recursion.

use super::*;

/// Whether a class about to be hoisted under `old_name` must take
/// the collision rename. Collision domain: everything visible at top
/// level once the hoist lands. Sibling entries already pushed onto
/// `top_names` shadow the check for the class's own occurrence, so
/// duplicates count. A unit-class candidate (`unit_shape`) collides
/// with any other class of the name anywhere in the program: the
/// reference census is keyed by name, and the rename gives it a name
/// only this class answers to.
///
/// EVERY hoisted class collides with a `let` / `function` / for-of
/// binding of its name anywhere. For a unit candidate that is the
/// census again (`const K = outer()` in the loop that evaluates
/// `outer` kept the class shared, rotation 667). For the shared lane
/// it is resolution: the class leaves nothing behind where it was
/// written, and a program-scope binding of the name takes that name
/// out of the class-object rewrite — `return [A, B]` inside `mk` read
/// the top level's own `const [A, B] = mk()`, still uninitialized
/// (`value is not a constructor`).
pub(super) fn name_collides(
    top_names: &[String],
    sibling_start: usize,
    old_name: &str,
    unit_shape: bool,
    name_counts: &super::hoist_nested_classes_census::NameCensus,
) -> bool {
    top_names.iter().filter(|n| *n == old_name).count() > 1
        || top_names[..sibling_start].iter().any(|n| n == old_name)
        || (unit_shape && name_counts.classes.get(old_name).copied().unwrap_or(0) > 1)
        || name_counts.bindings.contains_key(old_name)
}

/// Give the class at `stmts[idx]` its collision name and carry along
/// everything keyed by the old one: the display name, the declaration
/// text, the static `this` sites of its own bodies, and every reference
/// in the scope the name is visible in. Answers the new name. Moved out
/// of `walk_container` verbatim.
pub(super) fn apply_collision_rename(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    lifted: &mut [Stmt],
    idx: usize,
    old_name: &str,
    counter: &mut u32,
) -> String {
    let new_name = format!("__hc{}_{}", *counter, old_name);
    *counter += 1;
    // The rename is the hoist's, not the program's: `.name`,
    // the `[class X]` print and the instance's `X { … }` prefix
    // keep the source spelling, through the same table a named
    // class expression's binding uses (`class_display_name`).
    // A class expression that already carries a display name
    // carries it on. Before this `.name` answered `__hc0_D`.
    let display = ast
        .class_expr_display_names
        .get(old_name)
        .cloned()
        .unwrap_or_else(|| old_name.to_string());
    ast.class_expr_display_names
        .insert(new_name.clone(), display);
    // The declaration text (`String(C)`, §20.2.3.5) is keyed
    // by name too; a renamed class answered "" for it. The
    // span stays under the old name as well: a program with
    // two classes so named still has the other one.
    if let Some(span) = ast.class_decl_spans.get(old_name).copied() {
        ast.class_decl_spans.insert(new_name.clone(), span);
    }
    // The parser recorded, at the token, that a `this` in a
    // static body means THIS class — and it recorded it as the
    // NAME the source used, which `desugar_classes` pass 2
    // later mints. Renaming without remapping leaves those
    // sites naming a binding that no longer exists, and
    // `typeof` of an unresolvable name answers "undefined"
    // rather than saying so: the class renamed for a collision
    // starts answering wrong QUIETLY. Only the sites still
    // registered under the old name move, so a class nested
    // inside one of these bodies keeps its own.
    if let Stmt::ClassDecl { static_methods, .. } = &stmts[idx] {
        let sites: Vec<ExprId> = static_methods
            .iter()
            .flat_map(|m| super::capturing_classes::this_sites(ast, &m.body))
            .collect();
        for eid in sites {
            if ast
                .static_this_sites
                .get(&eid)
                .is_some_and(|c| *c == old_name)
            {
                ast.static_this_sites.insert(eid, new_name.clone());
            }
        }
    }
    rename_scope(ast, stmts, lifted, old_name, &new_name);
    new_name
}

/// Rename across the whole scope a container's class name is visible
/// in: the container's statements, and the siblings this container
/// already gave up to the hoist (`lifted`). A sibling declared BEFORE
/// the renamed class has left the container by the time the rename
/// runs, and its body still spells the old name — which, the rename
/// existing because another class shares it, is somebody else's
/// (`class E { make() { return new D() } } class D {}` in the second
/// of two functions declaring a `D`: "D is not defined").
pub(super) fn rename_scope(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    lifted: &mut [Stmt],
    old: &str,
    new: &str,
) {
    rename_in_stmts(ast, stmts, old, new);
    rename_in_stmts(ast, lifted, old, new);
}

pub(super) fn rename_in_stmts(ast: &mut Ast, stmts: &mut [Stmt], old: &str, new: &str) {
    for s in stmts.iter_mut() {
        rename_in_stmt(ast, s, old, new);
    }
}

fn rename_in_stmt(ast: &mut Ast, s: &mut Stmt, old: &str, new: &str) {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Yield(e) | Stmt::Throw(e) => {
            rename_in_expr(ast, *e, old, new)
        }
        Stmt::YieldInto { value, .. } => rename_in_expr(ast, *value, old, new),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            rename_in_expr(ast, *init, old, new)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rename_in_expr(ast, *cond, old, new);
            rename_in_stmt(ast, then_branch, old, new);
            if let Some(eb) = else_branch {
                rename_in_stmt(ast, eb, old, new);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rename_in_expr(ast, *cond, old, new);
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rename_in_stmt(ast, i, old, new);
            }
            for e in [cond.as_ref(), step.as_ref()].into_iter().flatten() {
                rename_in_expr(ast, *e, old, new);
            }
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rename_in_expr(ast, *parent, old, new);
            rename_in_expr(ast, *sep, old, new);
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            rename_in_expr(ast, *elem_expr, old, new);
            if let Some(o) = forin_obj {
                rename_in_expr(ast, *o, old, new);
            }
            rename_in_stmt(ast, body, old, new);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rename_in_expr(ast, *scrutinee, old, new);
            for c in cases {
                rename_in_expr(ast, c.value, old, new);
                rename_in_stmts(ast, &mut c.body, old, new);
            }
            if let Some(d) = default {
                rename_in_stmts(ast, d, old, new);
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => rename_in_stmts(ast, v, old, new),
        Stmt::Labeled { body, .. } => rename_in_stmt(ast, body, old, new),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rename_in_stmts(ast, body, old, new);
            rename_in_stmts(ast, catch_body, old, new);
            if let Some(fb) = finally_body {
                rename_in_stmts(ast, fb, old, new);
            }
        }
        Stmt::FnDecl { body, .. } => rename_in_stmts(ast, body, old, new),
        Stmt::ClassDecl {
            name,
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            // A computed key is evaluated in the scope the class is
            // written in (§15.7.14), so an outer name it reads renames
            // with that scope — and the keys live in a side table, not
            // under the ClassDecl. Not for the class being renamed
            // itself: there its own name in a key is the class-scope
            // binding, still in its TDZ.
            if name != old {
                let keys: Vec<ExprId> = ast
                    .class_computed_keys
                    .iter()
                    .filter(|((c, _), _)| c == name)
                    .map(|(_, e)| *e)
                    .collect();
                for k in keys {
                    rename_in_expr(ast, k, old, new);
                }
            }
            if name == old {
                *name = new.to_string();
            }
            // The heritage is an expression (RFC 20260815): a bare
            // `Expr::Ident` root is renamed by the same walk that
            // renames every identifier inside a non-Ident heritage.
            if let Some(pid) = parent {
                rename_in_expr(ast, *pid, old, new);
            }
            rename_in_bodies(ast, ctor, methods, static_methods, static_init, old, new);
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(inner) = inner {
                rename_in_stmt(ast, inner, old, new);
            }
            if let Some(e) = default_expr {
                rename_in_expr(ast, *e, old, new);
            }
        }
        Stmt::Return(None)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. } => {}
    }
}

/// Rename inside a class's own BODIES only — constructor, methods,
/// static methods and static initialisers. Not the `name` field, not
/// the heritage: §14.2.3 gives a class declaration two bindings, and
/// this walk covers exactly the half only the body can see.
pub(super) fn rename_in_class_bodies(ast: &mut Ast, s: &mut Stmt, old: &str, new: &str) {
    let Stmt::ClassDecl {
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return;
    };
    rename_in_bodies(ast, ctor, methods, static_methods, static_init, old, new);
}

fn rename_in_bodies(
    ast: &mut Ast,
    ctor: &mut Option<ClassCtor>,
    methods: &mut [ClassMethod],
    static_methods: &mut [ClassMethod],
    static_init: &mut [StaticInit],
    old: &str,
    new: &str,
) {
    if let Some(c) = ctor {
        rename_in_stmts(ast, &mut c.body, old, new);
    }
    for m in methods.iter_mut().chain(static_methods.iter_mut()) {
        rename_in_stmts(ast, &mut m.body, old, new);
    }
    for si in static_init {
        match si {
            StaticInit::Field(f) => rename_in_expr(ast, f.init, old, new),
            StaticInit::Block(v) => rename_in_stmts(ast, v, old, new),
        }
    }
}

fn rename_in_expr(ast: &mut Ast, eid: ExprId, old: &str, new: &str) {
    // Read the child ids out first so the arena borrow releases
    // before recursing.
    enum Kids {
        None,
        One(ExprId),
        Two(ExprId, ExprId),
        Three(ExprId, ExprId, ExprId),
        Many(Vec<ExprId>),
        Arrow,
    }
    let kids = match &mut ast.exprs[eid.0 as usize] {
        Expr::Ident(n) => {
            if n == old {
                *n = new.to_string();
            }
            Kids::None
        }
        Expr::New {
            class_name, args, ..
        } => {
            if class_name == old {
                *class_name = new.to_string();
            }
            Kids::Many(args.clone())
        }
        // The target is an expression now, so a bare class name in it
        // is an `Expr::Ident` the arm above already renames — nothing
        // to do here beyond descending into both children.
        Expr::InstanceOf { expr, rhs } => Kids::Two(*expr, *rhs),
        Expr::Elision
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Closure { .. } => Kids::None,
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => Kids::Two(*left, *right),
        Expr::Unary { expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::PostIncr { target: expr, .. } => Kids::One(*expr),
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            Kids::Many(v)
        }
        Expr::Super { args } => Kids::Many(args.clone()),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Kids::Three(*cond, *then_branch, *else_branch),
        Expr::Array(elems) => Kids::Many(elems.clone()),
        Expr::ObjectLit { fields } => Kids::Many(fields.iter().map(|(_, e)| *e).collect()),
        Expr::ArrowFn { .. } => Kids::Arrow,
    };
    match kids {
        Kids::None => {}
        Kids::One(a) => rename_in_expr(ast, a, old, new),
        Kids::Two(a, b) => {
            rename_in_expr(ast, a, old, new);
            rename_in_expr(ast, b, old, new);
        }
        Kids::Three(a, b, c) => {
            rename_in_expr(ast, a, old, new);
            rename_in_expr(ast, b, old, new);
            rename_in_expr(ast, c, old, new);
        }
        Kids::Many(v) => {
            for e in v {
                rename_in_expr(ast, e, old, new);
            }
        }
        Kids::Arrow => {
            let mut body = match &mut ast.exprs[eid.0 as usize] {
                Expr::ArrowFn { body, .. } => std::mem::take(body),
                _ => unreachable!(),
            };
            rename_in_stmts(ast, &mut body, old, new);
            if let Expr::ArrowFn { body: b, .. } = &mut ast.exprs[eid.0 as usize] {
                *b = body;
            }
        }
    }
}
