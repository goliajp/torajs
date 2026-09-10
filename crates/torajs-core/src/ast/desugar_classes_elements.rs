//! Duplicate class elements — the last definition wins.
//!
//! §15.7.14 ClassDefinitionEvaluation defines the elements one at a
//! time, in source order, each through DefineMethodProperty /
//! DefinePropertyOrThrow on the same key: a later method replaces
//! whatever the key held, a later getter replaces the getter of an
//! accessor pair and keeps its setter (and turns a data property into
//! an accessor), a later setter likewise. The desugar minted one
//! `__cm_<C>__<k>` FnDecl per element and the checker refused the
//! second as a redeclaration —
//!
//! ```text
//! class C2 { get b() { throw … } get ['b']() { return 'B' } }
//! new C2().b        // 'B'; tr: "redeclaration of function `__cm_C2__b_get`"
//! ```
//!
//! — which is exactly what test262 spells in
//! `computed-property-names/class/accessor/{getter,setter}-duplicates`.
//! This pass replays the definitions over each class's element list
//! (instance and static separately — they land on different objects)
//! and keeps only the survivors, in their original order, so every
//! later pass sees one declaration per key and kind.
//!
//! Gated on the Script goal like `dedupe_fn_decls`: on the `.ts` side
//! tsc reports TS2300 "Duplicate identifier" for a spelled-out
//! duplicate, and the checker's redeclaration refusal is the
//! spelling tr has for that (a computed key is indistinguishable from
//! a literal one by the time the key is a `PropKey`).
//!
//! Fields: every initializer runs, in order, whatever the key
//! (§15.7.14 step 31 walks the full list — the parser already turned
//! the list into ctor-prefix assignments in that order), so a
//! duplicate INSTANCE field is one layout slot with several writes:
//! the layout list keeps the last declaration per key and the writes
//! stay. A duplicate STATIC field is the same story on the class
//! object; `emit_static_inits` emits the later ones as assignments to
//! the slot the first one declared.

use std::collections::HashMap;

use super::{AccessorKind, Ast, ClassMethod, PropKey, Stmt};

pub(super) fn dedupe_class_elements(ast: &mut Ast) {
    if !ast.sloppy_script_goal {
        return;
    }
    for s in ast.stmts.iter_mut() {
        if let Stmt::ClassDecl {
            methods,
            static_methods,
            fields,
            ..
        } = s
        {
            dedupe_list(methods);
            dedupe_list(static_methods);
            dedupe_fields(fields);
        }
    }
}

/// What a key currently holds while the definitions are replayed.
enum Slot {
    Data(usize),
    Accessor {
        get: Option<usize>,
        set: Option<usize>,
    },
}

/// Replay the definitions of one element list and keep the survivors.
fn dedupe_list(methods: &mut Vec<ClassMethod>) {
    if methods.len() < 2 {
        return;
    }
    let mut slots: HashMap<PropKey, Slot> = HashMap::new();
    for (i, m) in methods.iter().enumerate() {
        let entry = slots.entry(m.name.clone());
        match m.accessor_kind {
            None => {
                entry.insert_entry(Slot::Data(i));
            }
            Some(kind) => {
                let slot = entry.or_insert(Slot::Accessor {
                    get: None,
                    set: None,
                });
                if let Slot::Data(_) = slot {
                    *slot = Slot::Accessor {
                        get: None,
                        set: None,
                    };
                }
                if let Slot::Accessor { get, set } = slot {
                    match kind {
                        AccessorKind::Getter => *get = Some(i),
                        AccessorKind::Setter => *set = Some(i),
                    }
                }
            }
        }
    }
    let mut keep = vec![false; methods.len()];
    for slot in slots.values() {
        match slot {
            Slot::Data(i) => keep[*i] = true,
            Slot::Accessor { get, set } => {
                if let Some(i) = get {
                    keep[*i] = true;
                }
                if let Some(i) = set {
                    keep[*i] = true;
                }
            }
        }
    }
    if keep.iter().all(|k| *k) {
        return;
    }
    let mut i = 0;
    methods.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}

/// One layout slot per key — the LAST declaration's annotation, the
/// spelling a reader of the class sees last (every declaration in a
/// Script is un-annotated anyway).
fn dedupe_fields(fields: &mut Vec<(PropKey, String)>) {
    if fields.len() < 2 {
        return;
    }
    let mut last: HashMap<PropKey, usize> = HashMap::new();
    for (i, (k, _)) in fields.iter().enumerate() {
        last.insert(k.clone(), i);
    }
    if last.len() == fields.len() {
        return;
    }
    let mut i = 0;
    fields.retain(|(k, _)| {
        let keep = last[k] == i;
        i += 1;
        keep
    });
}
