//! What a vtable slot is made of, and what it answers.
//!
//! Two things live here because they are the same question asked at
//! two depths: [`hierarchy_root`] decides which bodies share a slot
//! (a receiver only ever wears a row from its own chain), and
//! [`join_vtable_slot_returns`] decides what that shared slot
//! returns. The root walk is the population every slot rule reads —
//! `num_width::slot_abi` unions widths over it and
//! `ssa_lower_module_metadata_slot_abi` checks ABIs over it — so it
//! is defined once, here, where the `class_parents` it walks lives.
//!
//! ## 508-06 — one slot, one return type
//!
//! `ssa_lower_module_metadata_slot_abi` asserts that every body a
//! slot can hold calls the same way; this is the pass that makes
//! that true for the RETURN position, and it is the only place that
//! knows what the disagreement means. A slot's rows are the bodies
//! of one method name overridden inside one hierarchy — a call
//! through the slot is emitted with a single body's signature, so
//! two rows that answer differently-shaped values leave one of them
//! entered under the wrong ABI.
//!
//! Runs right after `desugar_implicit_generics`: that pass is where
//! an unannotated body's return type is decided, so before it the
//! rows have nothing to disagree about, and after it nothing else
//! rewrites a `__cm_` return annotation.

use std::collections::HashMap;

use super::{Ast, Stmt};

/// 508-06 — widen every row of a vtable slot to `any` when the rows
/// disagree about what they return.
///
/// The disagreement that motivated this is the commonest shape there
/// is: a base that returns a value and an override that does not
/// (`class A { f(){ return 1 } }` + `class B extends A { f(){
/// log("b") } }`). Falling out of a body is `return undefined`
/// (§10.2.1.4 step 11), so the two rows are not in conflict about
/// the language — only about the machine: one answers in a float
/// register, the other answers in none. The slot's honest type is
/// their join, and `any` is the spelling that holds a value and the
/// undefined at once — the same reason `desugar_implicit_generics`
/// reaches for it when a body has both a value return and a
/// reachable fall-through.
///
/// Textual disagreement, not shape disagreement, is the trigger: a
/// slot whose rows say `string` and `any` passes the emitter's shape
/// check (both are words) and still hands the caller bits it will
/// read as the wrong thing. Rows that agree are left alone, so a
/// hierarchy that overrides `number` with `number` keeps its narrow
/// slot and its narrow call.
pub fn join_vtable_slot_returns(ast: &mut Ast) {
    if ast.method_index.is_empty() {
        return;
    }
    let anns = collect_cm_return_anns(ast);
    let plan = plan_slot_returns(ast, &anns);
    if plan.is_empty() {
        return;
    }
    apply_plan(ast, &plan);
}

/// Return annotation of every top-level `FnDecl`, normalised so that
/// "no annotation" and an explicit `void` read as the same answer —
/// they are the same machine shape and the same language answer.
fn collect_cm_return_anns(ast: &Ast) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stmt in super::toplevel_stmts_flat(ast) {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = stmt
        {
            let ann = return_type
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("void");
            out.insert(name.clone(), ann.to_string());
        }
    }
    out
}

/// Names of the `__cm_` bodies whose slot has rows that disagree,
/// plus the `__dispatch_` stub of every method name involved.
///
/// The population mirrors `num_width::slot_abi::slot_unions` exactly
/// — same method names, same per-class name spellings (bare and
/// mono-suffixed), same grouping by hierarchy root. It has to: that
/// pass is what makes the widths of these rows agree, and a slot the
/// two passes disagree about is a slot nobody checks.
fn plan_slot_returns(ast: &Ast, anns: &HashMap<String, String>) -> HashMap<String, String> {
    let mut plan: HashMap<String, String> = HashMap::new();
    for (m, by_root) in slot_groups_by_name(ast) {
        let stub = dispatch_stub_names(anns, &m);
        // Per ROOT: a receiver only ever wears a row from its own
        // chain, so an unrelated class that happens to declare the
        // same name fills the shared slot index with its own body
        // under its own signature and no call site sees both.
        let mut heads: Vec<String> = Vec::new();
        let mut split = false;
        for rows in by_root.values() {
            let head = anns[&rows[0]].clone();
            if rows.iter().any(|r| anns[r] != head) {
                // Rows that really answer different things: the
                // slot's type is their join, and `any` is the
                // spelling that holds a value and the undefined at
                // once.
                for r in rows {
                    plan.insert(r.clone(), "any".to_string());
                }
                split = true;
            } else {
                heads.push(head);
            }
        }
        if stub.is_empty() {
            continue;
        }
        // A dispatcher is minted only for a name whose owners form
        // ONE chain, so `heads` holds one entry here in practice; a
        // second would mean the stub forwards to rows it cannot both
        // match, and the honest answer is the join.
        heads.sort();
        heads.dedup();
        let want = if split || heads.len() != 1 {
            "any".to_string()
        } else {
            // Rows agree and only the stub differs. That is not a
            // disagreement about what the slot answers — the stub
            // read the base owner's DECLARATION, before
            // `desugar_implicit_generics` gave the unannotated
            // bodies theirs, so it is simply stale. It adopts what
            // the rows say.
            //
            // Widening the slot instead would be catastrophic and
            // almost universal: an unannotated chain method's stub
            // says `void` for every one of them, so every hierarchy
            // in every program would join to `any` and drag in the
            // any world. Measured when this branch widened: a
            // two-class program's artifact went 35,121 -> 133,425 B.
            heads[0].clone()
        };
        if want == "any" {
            for rows in by_root.values() {
                for r in rows {
                    plan.insert(r.clone(), "any".to_string());
                }
            }
        }
        for d in stub {
            if anns[&d] != want {
                plan.insert(d, want.clone());
            }
        }
    }
    plan
}

/// Every vtable slot's rows, keyed by method name then hierarchy
/// root. This IS the population — same method names, same per-class
/// spellings (bare and mono-suffixed), same grouping by root — that
/// `num_width::slot_abi::slot_unions` unions widths over. It has to
/// be: that pass is what makes these rows' widths agree, and a slot
/// the two passes disagree about is a slot nobody checks.
fn slot_groups_by_name(ast: &Ast) -> Vec<(String, HashMap<String, Vec<String>>)> {
    let decls: Vec<String> = super::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut classes: Vec<&String> = ast.class_parents.keys().collect();
    classes.sort();
    let mut names: Vec<&String> = ast.method_index.keys().collect();
    names.sort();
    let mut out = Vec::new();
    for m in names {
        let mut by_root: HashMap<String, Vec<String>> = HashMap::new();
        for c in &classes {
            let prefix = format!("__cm_{c}__{m}");
            let mut cms: Vec<&String> = decls
                .iter()
                .filter(|k| {
                    k.strip_prefix(prefix.as_str())
                        .is_some_and(|r| r.is_empty() || r.starts_with("$$"))
                })
                .collect();
            cms.sort();
            let root = hierarchy_root(ast, c);
            for cm in cms {
                by_root.entry(root.clone()).or_default().push(cm.clone());
            }
        }
        out.push((m.clone(), by_root));
    }
    out
}

/// The `__dispatch_<M>` stub's name, plus its mono spellings (the
/// `$$` suffix rides the name's tail). Empty for a method with no
/// dispatcher — a name an unrelated class also declares gets none.
fn dispatch_stub_names(anns: &HashMap<String, String>, m: &str) -> Vec<String> {
    let d = format!("__dispatch_{m}");
    let mut out: Vec<String> = anns
        .keys()
        .filter(|k| {
            k.strip_prefix(d.as_str())
                .is_some_and(|r| r.is_empty() || r.starts_with("$$"))
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// Write each planned annotation onto its declaration. A body that
/// falls off its end needs no new `return` when it widens to `any`:
/// an `any` slot has a spelling for undefined, which is exactly why
/// the tail close can terminate it.
fn apply_plan(ast: &mut Ast, plan: &HashMap<String, String>) {
    fn walk(stmts: &mut [Stmt], plan: &HashMap<String, String>) {
        for s in stmts {
            match s {
                Stmt::Multi(inner) => walk(inner, plan),
                Stmt::FnDecl {
                    name, return_type, ..
                } => {
                    if let Some(a) = plan.get(name.as_str()) {
                        *return_type = Some(a.clone());
                    }
                }
                _ => {}
            }
        }
    }
    walk(&mut ast.stmts, plan);
}

/// Topmost ancestor of `c` along `class_parents` — the identity of
/// the chain a receiver's rows can come from. A monomorph row's
/// `$$<suffix>` tail is dropped first: a specialization sits in its
/// base class's chain, not in one of its own. Hop-bounded so a
/// malformed `extends` cycle terminates instead of hanging.
pub fn hierarchy_root(ast: &Ast, c: &str) -> String {
    let mut cur = c.split_once("$$").map(|(b, _)| b).unwrap_or(c).to_string();
    let mut hops = ast.class_parents.len() + 1;
    while let Some(p) = ast.class_parents.get(&cur).and_then(|p| p.clone()) {
        cur = p;
        hops -= 1;
        if hops == 0 {
            break;
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slot's rows are found by name, so a test only has to supply
    /// the three things the pass reads: who inherits from whom, which
    /// names took a slot, and what each body says it returns.
    fn ast_of(
        parents: &[(&str, Option<&str>)],
        slots: &[&str],
        fns: &[(&str, Option<&str>)],
    ) -> Ast {
        let mut ast = Ast::default();
        for (c, p) in parents {
            ast.class_parents
                .insert((*c).to_string(), p.map(str::to_string));
        }
        for (i, m) in slots.iter().enumerate() {
            ast.method_index.insert((*m).to_string(), i as u32);
        }
        for (name, ret) in fns {
            ast.stmts.push(Stmt::FnDecl {
                name: (*name).to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: ret.map(str::to_string),
                body: Vec::new(),
                is_generator: false,
                span: crate::lexer::Span { start: 0, end: 0 },
            });
        }
        ast
    }

    fn ret_of(ast: &Ast, want: &str) -> Option<String> {
        ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name, return_type, ..
            } if name == want => Some(return_type.clone().unwrap_or_else(|| "void".into())),
            _ => None,
        })
    }

    #[test]
    fn a_value_base_and_a_void_override_join_to_any() {
        // The motivating shape: falling out of `B.f` answers
        // undefined, which the slot has to be able to hold next to
        // `A.f`'s number.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["f"],
            &[("__cm_A__f", Some("number")), ("__cm_B__f", None)],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__cm_A__f").as_deref(), Some("any"));
        assert_eq!(ret_of(&ast, "__cm_B__f").as_deref(), Some("any"));
    }

    #[test]
    fn rows_that_agree_keep_their_narrow_type() {
        // Nothing to join: an override that answers the same shape
        // leaves the slot — and every call through it — narrow.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["m"],
            &[("__cm_A__m", Some("number")), ("__cm_B__m", Some("number"))],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__cm_A__m").as_deref(), Some("number"));
    }

    #[test]
    fn no_annotation_and_an_explicit_void_are_the_same_answer() {
        // Both spell "this body answers undefined"; disagreeing about
        // the spelling is not disagreeing about the slot.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["m"],
            &[("__cm_A__m", None), ("__cm_B__m", Some("void"))],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__cm_A__m").as_deref(), Some("void"));
        assert_eq!(ret_of(&ast, "__cm_B__m").as_deref(), Some("void"));
    }

    #[test]
    fn an_unrelated_declarer_is_not_in_the_slot() {
        // `Other` shares the slot INDEX but no call site sees both
        // rows, so its own body keeps its own type — the same per-root
        // population `num_width::slot_abi` unions.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A")), ("Other", None)],
            &["m"],
            &[
                ("__cm_A__m", Some("number")),
                ("__cm_B__m", Some("number")),
                ("__cm_Other__m", Some("string")),
            ],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__cm_Other__m").as_deref(), Some("string"));
        assert_eq!(ret_of(&ast, "__cm_A__m").as_deref(), Some("number"));
    }

    #[test]
    fn the_dispatch_stub_moves_with_the_rows_it_forwards_to() {
        // It took its annotation from the base owner and is unioned
        // into the same slot, so leaving it behind would put the
        // disagreement back one level up.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["f"],
            &[
                ("__cm_A__f", Some("number")),
                ("__cm_B__f", None),
                ("__dispatch_f", Some("number")),
            ],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__dispatch_f").as_deref(), Some("any"));
    }

    #[test]
    fn a_stub_that_disagrees_with_agreeing_rows_still_splits_the_slot() {
        // The stub's annotation came from the base owner's
        // DECLARATION, read before the unannotated bodies were given
        // theirs — so it can be the only thing in the slot that is
        // wrong, with every row agreeing with every other.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["f"],
            &[
                ("__cm_A__f", Some("number")),
                ("__cm_B__f", Some("number")),
                ("__dispatch_f", None),
            ],
        );
        join_vtable_slot_returns(&mut ast);
        // The stub adopts what the rows say; nothing widens. Widening
        // here would fire on nearly every class hierarchy there is —
        // an unannotated chain method's stub always says `void`.
        assert_eq!(ret_of(&ast, "__dispatch_f").as_deref(), Some("number"));
        assert_eq!(ret_of(&ast, "__cm_A__f").as_deref(), Some("number"));
        assert_eq!(ret_of(&ast, "__cm_B__f").as_deref(), Some("number"));
    }

    #[test]
    fn a_mono_row_sits_in_its_base_class_chain() {
        // `hierarchy_root` drops the `$$` tail first, so a
        // specialization joins with the rows it shares a slot with
        // rather than forming a chain of its own.
        let mut ast = ast_of(
            &[("A", None), ("B", Some("A"))],
            &["f"],
            &[("__cm_A__f$$_number", Some("number")), ("__cm_B__f", None)],
        );
        join_vtable_slot_returns(&mut ast);
        assert_eq!(ret_of(&ast, "__cm_A__f$$_number").as_deref(), Some("any"));
        assert_eq!(ret_of(&ast, "__cm_B__f").as_deref(), Some("any"));
    }

    #[test]
    fn the_root_walk_terminates_on_a_malformed_cycle() {
        let ast = ast_of(&[("A", Some("B")), ("B", Some("A"))], &[], &[]);
        assert!(matches!(hierarchy_root(&ast, "A").as_str(), "A" | "B"));
    }
}
