//! `super.<m>(args)` in a STATIC member body — a static method, a
//! static field initializer, a static block. The home object there is
//! the class, so the base is the parent CLASS and the target a static
//! method. Split out of `desugar_classes_super` (verbatim move) ahead
//! of giving these sites their receiver.

use super::desugar_classes_super::{ClassIndexEntry, nearest_declaring};
use super::super_collect::collect_supercall_in_stmt;
use super::{Ast, ClassMethod, Expr, ExprId, StaticInit, Stmt};

/// Rewrite every static-body `super.<m>(args)` site of the class
/// `names.0` (whose parent is `names.1`): `__sm_<owner>__<m>(args)`
/// or the receiver-aware kernel when the program can re-link a
/// prototype (`dynamic`, see `super_home_dynamic`).
pub(super) fn rewrite_static_super_calls(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    names: (&String, &String),
    bodies: (&[ClassMethod], &[StaticInit]),
    dynamic: bool,
) {
    let (cname, parent_name) = names;
    let (static_methods, static_init) = bodies;
    let mut static_sites: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    for m in static_methods {
        for s in &m.body {
            collect_supercall_in_stmt(ast, s, &mut static_sites);
        }
    }
    // 420-04 — a static field initializer and a static block are
    // static member bodies too: §15.7.14 runs both with the class
    // as receiver, so their home object is the class and
    // `super.m()` names the parent CLASS. Only the method list was
    // walked, so those two positions kept the parser's raw
    // `__supercall__<m>` marker and died on it at typecheck. (A
    // bare `super(...)` CALL is a different question — the grammar
    // confines SuperCall to derived constructors, and the class
    // lane refuses it in a static body on its own.)
    for si in static_init {
        match si {
            StaticInit::Field(f) => {
                collect_supercall_in_stmt(ast, &Stmt::Expr(f.init), &mut static_sites);
            }
            StaticInit::Block(v) => {
                for s in v {
                    collect_supercall_in_stmt(ast, s, &mut static_sites);
                }
            }
        }
    }
    for (eid, m_name, args) in static_sites {
        // Same degrade as the instance sites, with the static home
        // object: `super.m()` in a static body names the parent
        // CLASS, whose link is `Object.getPrototypeOf(C)`.
        if dynamic {
            let base = super::super_home_dynamic::mint_home_proto(ast, cname, true);
            let key = ast.add_expr(Expr::String(m_name.into()));
            let recv = ast.add_expr(Expr::Ident(cname.clone()));
            let pack = ast.add_expr(Expr::Array(args));
            let kernel = ast.add_expr(Expr::Ident("__torajs_super_prop_call".to_string()));
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee: kernel,
                args: vec![base, key, recv, pack],
            };
            continue;
        }
        let owner = nearest_declaring(class_index, parent_name, &m_name, true)
            .unwrap_or(parent_name.clone());
        let callee = ast.add_expr(Expr::Ident(format!("__sm_{owner}__{m_name}")));
        ast.exprs[eid.0 as usize] = Expr::Call { callee, args };
    }
}
