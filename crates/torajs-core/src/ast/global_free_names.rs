//! Unresolvable bare names under the sloppy goal — the read half of
//! the global environment record (§9.1.1 object environment record).
//!
//! Sloppy code is GLOBAL code: §9.1.1.4 GetBindingValue on the global
//! environment record's OBJECT record answers a name the global object
//! carries, and only throws ReferenceError when the object does not
//! have it either. tr resolves names at compile time, so a name it
//! cannot place is refused outright — correct for a module (§16.2.1.6
//! resolves every binding statically) and wrong here, where the object
//! half of the record is a runtime question:
//!
//! ```text
//! this.p = 1;   // global code: `this` IS the global object
//! p;            // §9.1.1.4 — the object has it, so this reads 1
//! ```
//!
//! 76 `language/statements/with` cases and the annexB `global-code`
//! family are built on exactly that equivalence. The `this` half is
//! the sibling pass [`super::toplevel_this`], which under this goal
//! initialises the top-level binding to the `globalThis` singleton;
//! this pass is the bare-name half.
//!
//! # What is rewritten
//!
//! Every arena `Ident` whose name the program never declares and that
//! is not a known builtin global becomes a call to the injected helper
//! — a plain TS function, the `desugar_with` helper convention, not an
//! intrinsic:
//!
//! ```text
//! p   ->   __torajs_global_get("p")
//! ```
//!
//! The scan is over the ARENA, not the tree: an `Ident` node carries no
//! context of its own, so the only thing the walk has to know is which
//! ExprIds sit in a position that is NOT a read. Those four positions
//! are arena-local too (an `Assign` names its target, a `TypeOf` its
//! operand), so a parent scan collects them without any tree shape.
//!
//! # What is NOT rewritten, and why each stays out
//!
//! - **names the program declares anywhere** — the set is the
//!   scope-BLIND one `delete_bare_name::collect_declared_names`
//!   builds, deliberately: a name declared in some other scope stays
//!   on the static lane, which is the conservative direction (the
//!   static lane is what tr does today);
//! - **known builtin globals** — `is_known_builtin_global` is the
//!   checker's own table, so this pass and the checker agree by
//!   construction on which names have a static lane;
//! - **writes** (`p = 1`) — an assignment to an unresolvable name
//!   CREATES the binding, and [`super::sloppy_implicit_globals`]
//!   already synthesizes a hoisted `var` for it. That pass runs FIRST
//!   and its synthesized names land in `declared`, so a name with a
//!   write keeps one storage for both its write and its read. Splitting
//!   them — write to the var, read through the object — is the one
//!   outcome this pass may not produce;
//! - **`typeof` / `delete` / `++`** — §13.5.1.1 and §13.5.1.2 answer
//!   an unresolvable name without throwing, so the guarded read is the
//!   wrong shape for them; they keep today's answer until the sibling
//!   knife covers them.

use super::{Ast, Expr, ExprId, Stmt};

/// §9.1.1.4 GetBindingValue with `S` = false, over the object half of
/// the global environment record. Spliced into the user arena through
/// the `parse_into` convention (`desugar_with::inject_helpers` is the
/// same move); the `__tglob_` prefix keeps the locals out of user
/// namespace, which a by-name census downstream would otherwise
/// collide with.
const HELPER_SRC: &str = r#"
function __torajs_global_get(__tglob_n: any): any {
  const __tglob_g: any = globalThis;
  if (__tglob_n in __tglob_g) { return __tglob_g[__tglob_n]; }
  throw new ReferenceError(__tglob_n + " is not defined");
}
"#;

const HELPER_FN: &str = "__torajs_global_get";

pub fn resolve_global_free_names(ast: &mut Ast) {
    if !ast.program_is_sloppy() {
        return;
    }
    let declared = declared_anywhere(ast);

    // Positions that are not reads. Collected from the arena itself —
    // every one of them is a parent node naming its own child.
    let mut not_a_read: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for e in &ast.exprs {
        let child = match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => *target,
            Expr::TypeOf { expr } | Expr::Delete { expr } => *expr,
            _ => continue,
        };
        not_a_read.insert(child.0);
    }

    let hits: Vec<(ExprId, String)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Ident(n) if !not_a_read.contains(&(i as u32)) && is_free(n, &declared) => {
                Some((ExprId(i as u32), n.clone()))
            }
            _ => None,
        })
        .collect();
    if hits.is_empty() {
        return;
    }
    inject_helper(ast);
    for (eid, name) in hits {
        let callee = ast.add_expr(Expr::Ident(HELPER_FN.to_string()));
        let arg = ast.add_expr(Expr::String(name.as_str().into()));
        ast.exprs[eid.0 as usize] = Expr::Call {
            callee,
            args: vec![arg],
        };
    }
}

/// The name has no static lane: the program declares it nowhere and it
/// is not a builtin the checker knows. Mirrors
/// `sloppy_implicit_globals::admit`'s exclusions — the two passes are
/// asking the same question about the same names, one for writes and
/// one for reads, and they must not disagree.
fn is_free(n: &str, declared: &std::collections::HashSet<String>) -> bool {
    if n.starts_with("__") {
        // Compiler-synthesized spellings (`__this`, the with helpers,
        // the harness ports). None of them is a user global.
        return false;
    }
    !(declared.contains(n)
        || crate::check::is_known_builtin_global(n)
        || matches!(
            n,
            "undefined"
                | "NaN"
                | "Infinity"
                | "eval"
                | "arguments"
                | "globalThis"
                | "let"
                | "yield"
                | "await"
                | "static"
                | "this"
                | "super"
        ))
}

/// Every name the program binds ANYWHERE, in any scope.
///
/// `collect_declared_names` walks the statement TREE, which reaches a
/// `function` declaration's body but not an arrow / function
/// EXPRESSION's — those hang off the arena, not off a statement. That
/// gap is harmless for the sibling that synthesizes a hoisted `var`
/// (an extra binding no one reads) and fatal here: a `var local`
/// inside a function expression would look unresolvable and its reads
/// would be sent to the global object, turning a working local into a
/// ReferenceError (gate: 31 such fixtures).
///
/// So the arena is scanned flat for every `ArrowFn` — the variant the
/// parser parks arrows, function expressions and object-literal
/// methods in — and each one's params and body statements join the
/// set. Flat is enough: a nested arrow is its own arena entry, so the
/// scan reaches every depth without recursing itself.
///
/// The set stays scope-BLIND on purpose (same recorded approximation
/// the sibling carries): it answers "the program binds this name
/// somewhere", and a name bound in some unrelated scope therefore
/// keeps the static lane. That is the conservative direction — the
/// static lane is what tr does today.
fn declared_anywhere(ast: &Ast) -> std::collections::HashSet<String> {
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    collect_class_bodies(&ast.stmts, &mut declared);
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            for p in params {
                declared.insert(p.name.clone());
            }
            super::delete_bare_name::collect_declared_names(body, &mut declared);
            collect_class_bodies(body, &mut declared);
        }
    }
    declared
}

/// A class body is the third place a binding hides. `collect_declared_
/// names` stops at `Stmt::ClassDecl` (its walk is the delete triage's,
/// which has no business inside one), so a method's params and locals,
/// and a `static { … }` block's, were invisible — the remaining 7 of
/// the 31 gate fixtures after the arena scan above, all of them class
/// shapes.
fn collect_class_bodies(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        if let Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } = s
        {
            // The constructor is its own field, not one of `methods`.
            if let Some(c) = ctor {
                for p in &c.params {
                    out.insert(p.name.clone());
                }
                super::delete_bare_name::collect_declared_names(&c.body, out);
                collect_class_bodies(&c.body, out);
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for p in &m.params {
                    out.insert(p.name.clone());
                }
                super::delete_bare_name::collect_declared_names(&m.body, out);
                collect_class_bodies(&m.body, out);
            }
            for init in static_init {
                if let super::StaticInit::Block(b) = init {
                    super::delete_bare_name::collect_declared_names(b, out);
                    collect_class_bodies(b, out);
                }
            }
        }
        for child in super::desugar_with::walk::stmt_children_ref(s) {
            collect_class_bodies(core::slice::from_ref(child), out);
        }
    }
}

fn inject_helper(ast: &mut Ast) {
    let src = HELPER_SRC.to_string();
    let tokens = crate::lexer::tokenize(&src).expect("global helper lex");
    let offset = crate::parser::parse_into(&src, &tokens, ast).expect("global helper parse");
    // Spans index HELPER_SRC, not the user's file — the (0,0) "no user
    // source" sentinel is the stmt.rs contract for an injected
    // declaration (`desugar_with::inject_helpers` stamps the same one).
    for s in ast.stmts[offset..].iter_mut() {
        if let Stmt::FnDecl { span, .. } = s {
            *span = crate::lexer::Span { start: 0, end: 0 };
        }
    }
    let injected: Vec<Stmt> = ast.stmts.split_off(offset);
    ast.stmts.splice(0..0, injected);
}
