//! `$262.evalScript(src)` — a Script evaluated from inside the program
//! (the test262 runner spells it `__t262_evalScript(src)` so this pass
//! can tell it from an indirect eval). §16.1.7
//! GlobalDeclarationInstantiation differs from §19.2.1.3 in exactly the
//! ways the `language/global-code/script-decl-*` cases probe:
//!
//! - a Script's `let` / `const` / `class` join the GLOBAL lexical
//!   environment — every later statement sees them — where an eval's
//!   stay inside the eval. So a Script inlines FLAT (a `Stmt::Multi`
//!   whose children share the surrounding scope, flattened into the
//!   top-level list afterwards), never as a `Block`;
//! - steps 3-5: a Script whose lexical names collide with an existing
//!   `var` / function / lexical binding, or whose `var` / function
//!   names collide with a lexical binding, throws a SyntaxError WHEN
//!   IT IS EVALUATED and creates no bindings at all. The program's own
//!   early-redeclaration check (`ast_early_redecl`) cannot host this:
//!   it is a compile-time reject, and `assert.throws(SyntaxError,
//!   function () { $262.evalScript('var x; let test262Let;') })` wants
//!   the throw at run time with `x` still undeclared afterwards.
//!
//! The collision check is a pre-pass in textual order over the whole
//! program, before any inlining, so it sees the program's declarations
//! as written and each Script's names as parsed. The same check covers
//! the other global `var` code — an indirect eval anywhere, a direct
//! eval at the top of a sloppy program — for the one collision
//! §19.2.1.3 step 5 gives them: a `var` over a global lexical binding. A colliding Script
//! becomes the same `throw new SyntaxError(...)` a non-parsing eval
//! becomes; a clean top-level Script adds its names to the registry
//! for the Scripts after it. Registered residue: a Script inside a
//! function body is walked after every top-level statement (arena
//! order), so a top-level Script textually below it is consulted as if
//! it ran first; and HasRestrictedGlobalProperty (a lexical name over a
//! non-configurable global property, `script-decl-lex-restricted-
//! global`) is a runtime fact this static check cannot see.

use std::collections::HashSet;

use super::super::desugar_with::walk;
use super::super::var_hoist::ScriptGlobalVar;
use super::super::{Ast, Expr, Stmt};
use super::recognize::{has_use_strict_prologue, literal_eval_call};
use super::source::{CallForm, EvalStrictness, parse_eval_source, syntax_error_throw};

/// The two name sets §16.1.7 keeps apart: LexicallyDeclaredNames and
/// VarDeclaredNames (the latter including function declarations).
#[derive(Default)]
struct Names {
    lex: HashSet<String>,
    vars: HashSet<String>,
}

impl Names {
    /// The names one statement list declares at ITS top level — a
    /// Script's own, or the program's.
    fn of(stmts: &[Stmt]) -> Names {
        let mut out = Names::default();
        let mut counts = std::collections::HashMap::new();
        super::super::var_hoist::count_var_names(stmts, &mut counts);
        out.vars.extend(counts.into_keys());
        for s in stmts {
            match s {
                Stmt::LetDecl {
                    name,
                    is_var: false,
                    ..
                }
                | Stmt::ClassDecl { name, .. } => {
                    out.lex.insert(name.clone());
                }
                Stmt::FnDecl { name, .. } => {
                    out.vars.insert(name.clone());
                }
                _ => {}
            }
        }
        out
    }

    /// The first name that may not be declared against `self` as the
    /// global environment, if any. A Script (§16.1.7 steps 3-5) checks
    /// its lexical names against every binding and its `var` names
    /// against the lexical ones; an eval (§19.2.1.3 step 5) has no
    /// global lexical names of its own — only its `var`s can collide.
    fn collision_for(&self, form: CallForm, code: &Names) -> Option<String> {
        let lex_clash = || {
            code.lex
                .iter()
                .find(|n| self.vars.contains(*n) || self.lex.contains(*n))
        };
        let var_clash = || code.vars.iter().find(|n| self.lex.contains(*n));
        match form {
            CallForm::Script => lex_clash().or_else(var_clash),
            CallForm::Direct | CallForm::Indirect => var_clash(),
        }
        .cloned()
    }

    fn absorb(&mut self, script: Names) {
        self.lex.extend(script.lex);
        self.vars.extend(script.vars);
    }
}

/// Replace every statement-position Script whose declarations collide
/// with the bindings already in force by the SyntaxError §16.1.7 throws
/// when it is evaluated. Runs before the value-position collapse and
/// the inlining walk, on the program as parsed.
pub(super) fn throw_colliding_scripts(ast: &mut Ast) {
    let mut reg = Names::of(&ast.stmts);
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in stmts.iter_mut() {
        visit(s, ast, &mut reg, false);
    }
    ast.stmts = stmts;
    // Function bodies written in expression position park their
    // statements in the arena (`rewrite_arrow_bodies` has the same
    // walk); each is a function scope.
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) else {
            i += 1;
            continue;
        };
        let mut taken = std::mem::take(body);
        for s in taken.iter_mut() {
            visit(s, ast, &mut reg, true);
        }
        if let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) {
            *body = taken;
        }
        i += 1;
    }
}

fn visit(s: &mut Stmt, ast: &mut Ast, reg: &mut Names, in_fn: bool) {
    if let Stmt::Expr(eid) = s
        && let Some((src, form)) = literal_eval_call(*eid, ast)
        && global_var_code(form, in_fn, ast)
    {
        let eid = *eid;
        let exprs_before = ast.exprs.len();
        // A text that does not parse is the inlining walk's step-12
        // throw, not a collision.
        let Ok(parsed) = parse_eval_source(&src, ast, false, EvalStrictness::Sloppy) else {
            return;
        };
        // A strict eval's `var`s are its own (§19.2.1.3 steps 3-5) —
        // nothing global to collide with. A Script's are global
        // whatever its strictness.
        if form != CallForm::Script && has_use_strict_prologue(&parsed, ast) {
            neutralize(ast, exprs_before);
            return;
        }
        let names = Names::of(&parsed);
        let redeclares_only = names.lex.is_empty()
            && parsed.iter().all(|st| match st {
                Stmt::LetDecl {
                    name,
                    init,
                    is_var: true,
                    ..
                } => reg.vars.contains(name) && matches!(ast.exprs[init.0 as usize], Expr::Uninit),
                _ => false,
            });
        // The parse was for its names only: the statements are dropped
        // and their expressions must not stay in the arena for the
        // arena-walking passes to find (`source::neutralize`).
        neutralize(ast, exprs_before);
        if let Some(clash) = reg.collision_for(form, &names) {
            ast.exprs[eid.0 as usize] = Expr::Ident("undefined".to_string());
            *s = syntax_error_throw(
                format!("Identifier '{clash}' has already been declared"),
                ast,
            );
        } else if in_fn && redeclares_only {
            // Global code the inlining walk cannot place (it sits in a
            // function) whose every declaration re-declares an existing
            // global `var`: §16.1.7 CanDeclareGlobalVar says do nothing,
            // so the call completes with `undefined` and nothing else.
            ast.exprs[eid.0 as usize] = Expr::Ident("undefined".to_string());
        } else if !in_fn && form == CallForm::Script {
            // A Script's declarations join the global environment for
            // the Scripts after it; an eval's `var`s do not block a
            // later Script's lexical declaration (`script-decl-lex-var-
            // declared-via-eval`).
            reg.absorb(names);
        }
        return;
    }
    let enters_fn = matches!(s, Stmt::FnDecl { .. });
    for c in walk::stmt_children(s) {
        visit(c, ast, reg, in_fn || enters_fn);
    }
}

/// Is this call's text global `var` code — declarations that land on
/// the global environment and so can collide with it? A Script always;
/// an indirect eval always (§19.2.1: global, sloppy unless its own
/// prologue says otherwise — checked on the parse); a direct eval only
/// at the top level of a sloppy program, where the caller's variable
/// environment IS the global one.
fn global_var_code(form: CallForm, in_fn: bool, ast: &Ast) -> bool {
    match form {
        CallForm::Script | CallForm::Indirect => true,
        CallForm::Direct => !in_fn && ast.program_is_sloppy(),
    }
}

/// Blank the expressions a names-only parse appended (`source::
/// neutralize`, which stays private to its module).
fn neutralize(ast: &mut Ast, exprs_before: usize) {
    for e in ast.exprs[exprs_before..].iter_mut() {
        *e = Expr::Bool(false);
    }
}

/// Splice every top-level `Stmt::Multi` into the list it sits in. A
/// Multi's children share the surrounding scope by definition, so the
/// program means the same; what changes is that the passes reading the
/// top-level list directly — `toplevel_fn_decl_names`, the `var`
/// hoist's function pre-binding, `script_global_fns` — now see a
/// Script's (or a sloppy eval's) function declarations as the top-level
/// declarations they are.
pub(super) fn flatten_top_multi(stmts: Vec<Stmt>) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            Stmt::Multi(v) => out.extend(flatten_top_multi(v)),
            other => out.push(other),
        }
    }
    out
}

/// §16.1.7 step 8 (and §19.2.1.3 step 8): of several top-level
/// function declarations with one name, the LAST is the one
/// instantiated. Under the Script goal a duplicate is legal — function
/// declarations are VarDeclaredNames there, not lexical — so the
/// earlier ones are dropped before the checker, which refuses a
/// duplicate as a module would. Until the flat splice above, a Script's
/// or eval's declarations sat under a Multi and were not top-level
/// declarations at all.
pub(super) fn keep_last_fn_decls(stmts: &mut Vec<Stmt>) {
    let mut seen = HashSet::new();
    let mut keep: Vec<bool> = Vec::with_capacity(stmts.len());
    for s in stmts.iter().rev() {
        keep.push(match s {
            Stmt::FnDecl { name, .. } => seen.insert(name.clone()),
            _ => true,
        });
    }
    keep.reverse();
    let mut i = 0;
    stmts.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}

/// Record the `var` sites an inlined (unsealed) text declares, by the
/// declaration's `init` slot — fresh per site, so a name the program
/// also declares keeps its own sites apart. An eval's `var` is a
/// deletable global (§19.2.1.3), a Script's is not (§16.1.7); both bind
/// their property where the text ran (`var_hoist_eval_sites`).
pub(super) fn record_eval_var_sites(inlined: &[Stmt], ast: &mut Ast, form: CallForm) {
    let flavor = match form {
        CallForm::Script => ScriptGlobalVar::ScriptVar,
        CallForm::Direct | CallForm::Indirect => ScriptGlobalVar::EvalVar,
    };
    let mut counts = std::collections::HashMap::new();
    super::super::var_hoist::count_var_names(inlined, &mut counts);
    for sites in counts.into_values() {
        for init in sites {
            ast.eval_var_sites.insert(init, flavor);
        }
    }
}
