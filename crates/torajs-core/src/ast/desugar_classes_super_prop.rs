//! SuperProperty rewrite pass (§13.3.7 MakeSuperPropertyReference).
//!
//! Runs right after `rewrite_super_method_calls` (Pass 1.6) and before
//! Pass 2 (`Expr::This` rewrite). For every class with an `extends`
//! clause it walks the ctor + method bodies (instance context) and the
//! static-method bodies (static context) for the `__superbase__`
//! marker sites the parser encoded (see `super_collect_prop`) and
//! rewrites each per its role:
//!
//!   - read `super.x`, statically-declared getter on the chain →
//!     `__cm_<owner>__<x>_get(this)` (static ctx: `__sm_..._get()`),
//!     keeping the §13.3.7 receiver (`this`), which a plain member
//!     read off the prototype object could not deliver;
//!   - read `super[k]` → `__torajs_super_prop_get(<base>, k,
//!     <receiver>)`, the receiver-aware [[Get]]: a computed key can
//!     name an accessor no static walk could have found, and reading
//!     it off the base would run its getter against the BASE;
//!   - read `super.x`, anything else → a member read off the super
//!     base: `<Parent>.prototype` for instance members, the
//!     `<Parent>` class object for statics. The runtime prototype
//!     chain handles shadowing, plain data properties added at run
//!     time (`A.prototype.p = v`), method values, and undefined;
//!   - write `super.x = v` at statement position with a statically-
//!     declared setter on the chain → `__cm_<owner>__<x>_set(this, v)`;
//!   - write `super.x = v` / `super[k] = v` otherwise → a write to
//!     `this` (§9.1.9 OrdinarySet with receiver = this: data
//!     properties land on the receiver, and the member-assign lower's
//!     own accessor dispatch on `this` covers the un-shadowed
//!     inherited-setter shape).
//!
//!   - call `super[k](args)` → the whole site becomes
//!     `__torajs_super_prop_call(<base>, k, <receiver>, [args…])`,
//!     one kernel that reads the base with the §13.3.7 receiver and
//!     then invokes with it. Splitting it into a callee read plus a
//!     call would hand the BASE to the method — the right answer for
//!     `super.toString()` and silently wrong for anything reading
//!     `this`, which is why the call form does not travel the
//!     `IndexRead` path.
//!
//!   - update `super.x++` / `++super[k]` / `--` → the whole node
//!     becomes `__torajs_super_prop_{incr,decr}_{post,pre}(<base>, k,
//!     <receiver>)`: one kernel reads with the receiver, steps per
//!     ToNumeric, writes onto the receiver and answers the old / new
//!     value (rotation 628; the fused read/write was a recorded loud
//!     boundary before).
//!
//! Recorded approximation boundaries (loud, not silent — the marker
//! survives to the checker as an unknown ident): `super.x?.()`
//! optional calls.
//! Recorded silent boundaries: a RUNTIME-defined accessor on the base
//! (defineProperty after class definition) reads through the plain
//! member path with the prototype as receiver — for the NAME form
//! only; the computed form takes the kernel and is exact; a setter shadowed by
//! the subclass's own same-name setter resolves via `this` on the
//! fallback write path.
//!
//! Consumed marker nodes are overwritten with `Expr::Null` so no
//! orphan `__superbase__` survives in the arena for later
//! arena-scanning passes to trip over (rotation 333 blade 7 posture).

use super::desugar_classes_super::ClassIndexEntry;
use super::super_collect_prop::{
    SuperPropSite, SuperPropSites, collect_superprop_in_expr, collect_superprop_in_stmt,
    delete_operands,
};
use super::{AccessorKind, Ast, ClassMethod, Expr, ExprId, StaticInit};

/// What the first chain class declaring `name` says about it.
struct DeclaredShape {
    owner: String,
    has_getter: bool,
    has_setter: bool,
}

pub(super) fn rewrite_super_prop_sites(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    dynamic: bool,
) {
    for (_, cname, _tp, parent, _, static_inits, ctor, methods, static_methods) in class_index {
        // Instance context: ctor body (field inits are folded in) +
        // instance methods. Super base = <Parent>.prototype, receiver
        // spelling = `this` (Pass 2 rewrites it to `__this`). A class
        // WITHOUT an extends clause still has a super base in its
        // methods — `Object.prototype` (§10.2.4: the home object is
        // C.prototype, whose [[Prototype]] is %Object.prototype%), so
        // `class A { m() { return super.toString(); } }` reads there.
        // 508-04 — a class whose builtin heritage was STRIPPED has no
        // `parent` left, and spelling its super base `Object.prototype`
        // sends every computed form to the wrong object: `class MySet
        // extends Set { m(){ return super["has"](x) } }` answered
        // `super property is not a function` because `has` is not there.
        // The strip records who the parent was, and reading a method
        // off the builtin prototype works (`Set.prototype.has.call(this,
        // x)` is measured-good), so that is the base. The NAME forms
        // keep their own `__superbuiltin__` re-dispatch route — this
        // lane never sees them.
        let stripped = ast.builtin_class_parents.get(cname).cloned();
        let parent_name = parent.as_deref().or(stripped.as_deref());
        let mut inst = SuperPropSites::default();
        if let Some(c) = ctor.as_ref() {
            for s in &c.body {
                collect_superprop_in_stmt(ast, s, &mut inst);
            }
        }
        for m in methods {
            for s in &m.body {
                collect_superprop_in_stmt(ast, s, &mut inst);
            }
        }
        // 507-03 — a class whose heritage was STRIPPED to a builtin
        // (`exotic_parent`) keeps its base spelling: the runtime base
        // there is the builtin prototype, which this lane's kernels
        // cannot read a method off in the first place, so re-spelling
        // it would only move an already-recorded failure.
        let dyn_here = dynamic && !ast.exotic_parent.contains_key(cname);
        rewrite_sites(ast, class_index, parent_name, cname, false, dyn_here, inst);

        // Static context: super base = the <Parent> class object, or
        // %Function.prototype% for a base class (§10.2.4: the static
        // home object is C itself, whose [[Prototype]] is
        // %Function.prototype% — `Function.prototype` reads it, and a
        // write lands on the class object either way). Static blocks
        // and static field initializers run in the SAME context
        // (§15.7.14 step 31, `this` = C), so their sites join the
        // static methods' — rotation 628; before it only the method
        // bodies were walked and `static { v = super.x }` reached the
        // checker as an unknown ident.
        let mut stat = SuperPropSites::default();
        for m in static_methods {
            for s in &m.body {
                collect_superprop_in_stmt(ast, s, &mut stat);
            }
        }
        for si in static_inits {
            match si {
                StaticInit::Field(sf) => collect_superprop_in_expr(ast, sf.init, &mut stat),
                StaticInit::Block(stmts) => {
                    for s in stmts {
                        collect_superprop_in_stmt(ast, s, &mut stat);
                    }
                }
            }
        }
        rewrite_sites(ast, class_index, parent_name, cname, true, dyn_here, stat);
    }
}

fn rewrite_sites(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    parent_name: Option<&str>,
    cname: &str,
    is_static: bool,
    dynamic: bool,
    found: SuperPropSites,
) {
    let deletes = delete_operands(ast);
    for site in found.sites {
        match site {
            SuperPropSite::Read { member, name } => {
                // 507-03 — the static accessor call names the owner the
                // chain had at class-definition time; under a runtime
                // base the chain answers instead, so the name form
                // takes the same receiver-aware [[Get]] the computed
                // form does. That also retires this arm's recorded
                // silent boundary (a runtime-defined accessor read with
                // the BASE as receiver) for degraded programs.
                let decl = if dynamic {
                    None
                } else {
                    nearest_declaring_shape(class_index, parent_name, &name, is_static)
                };
                if let Some(d) = decl.filter(|d| d.has_getter) {
                    let callee =
                        ast.add_expr(Expr::Ident(accessor_fn(is_static, &d.owner, &name, "_get")));
                    let args = if is_static {
                        Vec::new()
                    } else {
                        vec![ast.add_expr(Expr::This)]
                    };
                    ast.exprs[member.0 as usize] = Expr::Call { callee, args };
                } else if deletes.contains(&member) {
                    // §13.5.1.2 step 4.b — recorded for the lowering,
                    // which no longer sees the `__superbase__` marker.
                    ast.super_delete_operands.insert(member);
                    let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                    ast.exprs[member.0 as usize] = Expr::Member { obj: base, name };
                } else if dynamic {
                    // See `delete_operands`: a delete operand must stay
                    // a property reference, so it keeps the plain read.
                    let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                    let key = ast.add_expr(Expr::String(name.into()));
                    let recv = mint_receiver(ast, cname, is_static);
                    let callee = ast.add_expr(Expr::Ident("__torajs_super_prop_get".to_string()));
                    ast.exprs[member.0 as usize] = Expr::Call {
                        callee,
                        args: vec![base, key, recv],
                    };
                } else {
                    let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                    ast.exprs[member.0 as usize] = Expr::Member { obj: base, name };
                }
            }
            SuperPropSite::IndexRead { index_expr } => {
                let Expr::Index { index, .. } = ast.get_expr(index_expr) else {
                    continue;
                };
                let key = *index;
                let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                if deletes.contains(&index_expr) {
                    // See `delete_operands` — the plain index stays,
                    // and §13.5.1.2 step 4.b is recorded for lowering.
                    ast.super_delete_operands.insert(index_expr);
                    ast.exprs[index_expr.0 as usize] = Expr::Index {
                        obj: base,
                        index: key,
                    };
                    continue;
                }
                let recv = mint_receiver(ast, cname, is_static);
                let callee = ast.add_expr(Expr::Ident("__torajs_super_prop_get".to_string()));
                ast.exprs[index_expr.0 as usize] = Expr::Call {
                    callee,
                    args: vec![base, key, recv],
                };
            }
            SuperPropSite::AssignName {
                assign,
                target,
                name,
                value,
                stmt_pos,
            } => {
                // 507-03 — same reason as the Read arm: under a runtime
                // base the static owner walk names an owner the chain
                // may no longer reach, so the site takes the
                // receiver-write fallback (§9.1.9 OrdinarySet with
                // receiver = this). Recorded boundary (507-05, not a
                // super question): that fallback's own accessor
                // dispatch is itself resolved through the class
                // hierarchy at compile time, so a setter only the
                // relinked chain declares is missed there too —
                // exactly as a plain `this.x = v` misses it.
                let decl = if dynamic {
                    None
                } else {
                    nearest_declaring_shape(class_index, parent_name, &name, is_static)
                };
                if stmt_pos && decl.as_ref().is_some_and(|d| d.has_setter) {
                    let owner = decl.unwrap().owner;
                    let callee =
                        ast.add_expr(Expr::Ident(accessor_fn(is_static, &owner, &name, "_set")));
                    let args = if is_static {
                        vec![value]
                    } else {
                        let this = ast.add_expr(Expr::This);
                        vec![this, value]
                    };
                    ast.exprs[assign.0 as usize] = Expr::Call { callee, args };
                } else {
                    let recv = mint_receiver(ast, cname, is_static);
                    ast.exprs[target.0 as usize] = Expr::Member { obj: recv, name };
                }
            }
            SuperPropSite::CallIndex { call, index_expr } => {
                let Expr::Call { args, .. } = ast.get_expr(call) else {
                    continue;
                };
                let args = args.clone();
                let Expr::Index { index, .. } = ast.get_expr(index_expr) else {
                    continue;
                };
                let key = *index;
                let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                let recv = mint_receiver(ast, cname, is_static);
                // The pack is an array literal, the protocol
                // `__torajs_super_value` already uses: one dense
                // `Arr<Any>` carries a spread element without the
                // kernel needing a spread protocol of its own.
                let pack = ast.add_expr(Expr::Array(args));
                let callee = ast.add_expr(Expr::Ident("__torajs_super_prop_call".to_string()));
                ast.exprs[call.0 as usize] = Expr::Call {
                    callee,
                    args: vec![base, key, recv, pack],
                };
                // The Index node the marker hung off is now orphaned
                // — same posture as the markers themselves.
                ast.exprs[index_expr.0 as usize] = Expr::Null;
            }
            SuperPropSite::AssignIndex { target_index } => {
                let Expr::Index { index, .. } = ast.get_expr(target_index) else {
                    continue;
                };
                let index = *index;
                let recv = mint_receiver(ast, cname, is_static);
                ast.exprs[target_index.0 as usize] = Expr::Index { obj: recv, index };
            }
            SuperPropSite::Update {
                node,
                target,
                is_inc,
                is_prefix,
            } => {
                let key = match ast.get_expr(target) {
                    Expr::Member { name, .. } => {
                        let name = name.clone();
                        ast.add_expr(Expr::String(name.into()))
                    }
                    Expr::Index { index, .. } => *index,
                    _ => continue,
                };
                let base = mint_base(ast, parent_name, cname, is_static, dynamic);
                let recv = mint_receiver(ast, cname, is_static);
                let callee = ast.add_expr(Expr::Ident(update_fn(is_inc, is_prefix).to_string()));
                ast.exprs[node.0 as usize] = Expr::Call {
                    callee,
                    args: vec![base, key, recv],
                };
                // The Member / Index the marker hung off is orphaned
                // — same posture as the markers themselves.
                ast.exprs[target.0 as usize] = Expr::Null;
            }
        }
    }
    for m in found.markers {
        ast.exprs[m.0 as usize] = Expr::Null;
    }
}

/// The update kernel's four spellings — the flags ride the callee
/// name so the call lowering appends them as constants.
fn update_fn(is_inc: bool, is_prefix: bool) -> &'static str {
    match (is_inc, is_prefix) {
        (true, false) => "__torajs_super_prop_incr_post",
        (true, true) => "__torajs_super_prop_incr_pre",
        (false, false) => "__torajs_super_prop_decr_post",
        (false, true) => "__torajs_super_prop_decr_pre",
    }
}

fn accessor_fn(is_static: bool, owner: &str, name: &str, suffix: &str) -> String {
    if is_static {
        format!("__sm_{owner}__{name}{suffix}")
    } else {
        format!("__cm_{owner}__{name}{suffix}")
    }
}

fn mint_base(
    ast: &mut Ast,
    parent_name: Option<&str>,
    cname: &str,
    is_static: bool,
    dynamic: bool,
) -> ExprId {
    // 507-03 — the parent is only the home object's prototype while
    // nothing re-links it; when the program can, the base is read from
    // the home object itself. That also covers the no-extends case
    // uniformly: `Object.getPrototypeOf(C.prototype)` IS
    // %Object.prototype% there.
    if dynamic {
        return super::super_home_dynamic::mint_home_proto(ast, cname, is_static);
    }
    // No extends clause → the instance super base is %Object.prototype%
    // and the static one %Function.prototype% (both read as the
    // builtin prototype singleton off the namespace).
    match (parent_name, is_static) {
        (Some(p), true) => ast.add_expr(Expr::Ident(p.to_string())),
        (Some(p), false) => {
            let parent = ast.add_expr(Expr::Ident(p.to_string()));
            ast.add_expr(Expr::Member {
                obj: parent,
                name: "prototype".to_string(),
            })
        }
        (None, is_static) => {
            let ns = ast.add_expr(Expr::Ident(
                if is_static { "Function" } else { "Object" }.to_string(),
            ));
            ast.add_expr(Expr::Member {
                obj: ns,
                name: "prototype".to_string(),
            })
        }
    }
}

fn mint_receiver(ast: &mut Ast, cname: &str, is_static: bool) -> ExprId {
    if is_static {
        ast.add_expr(Expr::Ident(cname.to_string()))
    } else {
        ast.add_expr(Expr::This)
    }
}

/// Walk `start` and its ancestors for the first class declaring `name`
/// in the relevant member list (instance methods or statics), and
/// report whether that declaration carries a getter / setter face.
/// The FIRST declaring class decides — a plain method there shadows
/// any accessor further up, exactly like the runtime chain would.
fn nearest_declaring_shape(
    class_index: &[ClassIndexEntry],
    start: Option<&str>,
    name: &str,
    is_static: bool,
) -> Option<DeclaredShape> {
    let mut cur = start?.to_string();
    for _ in 0..64 {
        let entry = class_index.iter().find(|e| e.1 == cur)?;
        let list: &[ClassMethod] = if is_static { &entry.8 } else { &entry.7 };
        let declared: Vec<&ClassMethod> = list.iter().filter(|m| m.name == name).collect();
        if !declared.is_empty() {
            return Some(DeclaredShape {
                owner: cur,
                has_getter: declared
                    .iter()
                    .any(|m| m.accessor_kind == Some(AccessorKind::Getter)),
                has_setter: declared
                    .iter()
                    .any(|m| m.accessor_kind == Some(AccessorKind::Setter)),
            });
        }
        cur = entry.3.clone()?;
    }
    None
}
