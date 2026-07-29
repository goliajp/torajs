//! M5.N — builtin heritage: `class C extends Object` (§19.1.1).
//!
//! A parent name that is not any declared class but names a
//! subclassable builtin is not a forward reference. The Object
//! constructor is explicitly designed to be subclassable (spec
//! §19.1.1), and under an active newTarget its [[Construct]] is
//! exactly OrdinaryCreateFromConstructor — which is what tr's
//! base-class factory already does (fresh instance, prototype chain
//! `C.prototype` → `Object.prototype`, `instanceof Object` true).
//! So the class lowers as a BASE class, with two seams handled
//! here before any other class pass runs:
//!
//! - `super(...)` sites in an explicit ctor rewrite to a comma
//!   chain evaluating the arguments left-to-right for effects
//!   (§13.3.7.1 ArgumentListEvaluation still runs — a poisoned
//!   argument still throws), result `undefined`; Object contributes
//!   no per-instance state.
//! - an explicit ctor with ZERO super() sites gets the
//!   `__torajs_ctor_no_super_throw()` raiser appended (§9.2.2
//!   this-TDZ, the `append_no_super_throw` shape) — that pass keys
//!   on `parent.is_some()` and would skip the stripped entry.
//!
//! Recorded boundaries (loud or registered, not silent-new):
//! `super.m()` in a stripped class keeps its `__supercall__` spelling
//! and fails loudly downstream; the class object's own [[Prototype]]
//! (spec: `Object.getPrototypeOf(C) === Object`) stays the base-class
//! shape; ctor return-override semantics (§9.2.2 step 13) are the
//! same pre-existing face user derived classes have.
//!
//! Array / RegExp / Promise / Iterator parents each need their own
//! exotic-instance substrate and join this table one by one.

use super::desugar_classes_super::ClassIndexEntry;
use super::super_collect::collect_super_in_stmt;
use super::*;

/// Builtins accepted as an `extends` parent today.
const SUBCLASSABLE_BUILTINS: &[&str] = &["Object"];

/// Strip a builtin parent down to base-class shape (see module doc).
/// Runs on the mutable `class_index` FIRST — before default-ctor
/// synthesis (a stripped class takes the base default ctor, not the
/// derived super-forwarding one) and before the forward-reference
/// validation in `compute_full_fields` (which would reject the
/// builtin name).
pub(super) fn strip_builtin_heritage(ast: &mut Ast, class_index: &mut [ClassIndexEntry]) {
    let declared: std::collections::HashSet<String> =
        class_index.iter().map(|e| e.1.clone()).collect();
    for (_, cname, _tp, parent, _, _, ctor, _, _) in class_index.iter_mut() {
        let Some(p) = parent.as_ref() else { continue };
        // A user class of the same name shadows the builtin — the
        // ordinary declared-parent path handles it.
        if declared.contains(p) || !SUBCLASSABLE_BUILTINS.contains(&p.as_str()) {
            continue;
        }
        if let Some(c) = ctor.as_mut() {
            let mut sites: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
            for s in &c.body {
                collect_super_in_stmt(ast, s, &mut sites);
            }
            if sites.is_empty() && ast.explicit_ctor_classes.contains(cname) {
                let callee = ast.add_expr(Expr::Ident("__torajs_ctor_no_super_throw".to_string()));
                let call = ast.add_expr(Expr::Call {
                    callee,
                    args: Vec::new(),
                });
                c.body.push(Stmt::Expr(call));
            }
            for (eid, args) in sites {
                if args.is_empty() {
                    ast.exprs[eid.0 as usize] = Expr::Ident("undefined".into());
                    continue;
                }
                let mut right = ast.add_expr(Expr::Ident("undefined".into()));
                for &a in args[1..].iter().rev() {
                    right = ast.add_expr(Expr::Sequence { left: a, right });
                }
                ast.exprs[eid.0 as usize] = Expr::Sequence {
                    left: args[0],
                    right,
                };
            }
        }
        *parent = None;
    }
}
