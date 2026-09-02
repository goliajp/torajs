//! Pass 0.5 of `lower_inner` — register user-declared type aliases, run
//! the V3-05 two-phase TypeDecl resolution (placeholder sids → fill +
//! intern), assign each declared class a runtime tag (Phase H.1.b), and
//! init the mutable interner state threaded through Pass 1 / 2
//! (`aliases`, `arr_layouts`, `fn_sigs`, `baked_regex_buf`, `inst_memo`,
//! `generic_struct_decls`, `struct_layouts`, `class_sids`,
//! `class_name_to_tag`). Also folds in the `may_throw` fixed-point
//! collection that immediately preceded the TypeDecl walks in the
//! original Pass 0.5 block.
//!
//! Extracted from `lower_inner` to drain the ~186-LOC Pass 0.5 region
//! out of the god-fn (chunk-328 of the lower_inner RFC decomp, after
//! Pass 0 batches A-D in chunks 323-326). Pure mechanical move: the
//! returned [`Pass05`] holder hands every binding back to `lower_inner`
//! by name, substrate codegen is invariant (binary byte-identical with
//! chunk-326 baseline 59444912).

use crate::ast::PropKey;
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, Module, Type};
use crate::ssa_lower_parse_type::{parse_struct_field_type, parse_type};

pub(crate) struct Pass05 {
    pub aliases: HashMap<String, Type>,
    pub arr_layouts: Vec<Type>,
    pub baked_regex_buf: Vec<BakedRegexEntry>,
    pub fn_sigs: Vec<(Vec<Type>, Type)>,
    pub may_throw: HashSet<String>,
    pub generic_struct_decls: HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    pub struct_layouts: Vec<Vec<(PropKey, Type)>>,
    pub inst_memo: HashMap<String, ssa::StructId>,
    pub class_name_to_tag: HashMap<String, u32>,
}

pub(crate) fn run(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    module: &mut Module,
    num_f64_slots: &WidthTable,
) -> Pass05 {
    let mut aliases: HashMap<String, Type> = HashMap::new();
    let mut arr_layouts: Vec<Type> = Vec::new();
    let baked_regex_buf: Vec<BakedRegexEntry> = Vec::new();
    let mut fn_sigs: Vec<(Vec<Type>, Type)> = Vec::new();
    let may_throw = crate::ast_throw_info::compute_may_throw_fns(ast, expr_types);

    let mut generic_struct_decls: HashMap<String, (Vec<String>, Vec<(String, String)>)> =
        HashMap::new();
    let mut struct_layouts: Vec<Vec<(PropKey, Type)>> = std::mem::take(&mut module.struct_layouts);
    let mut inst_memo: HashMap<String, ssa::StructId> = HashMap::new();
    let mut class_sids: std::collections::HashMap<String, ssa::StructId> =
        std::collections::HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() {
                continue;
            }
            // V3-18 wedge — bare type alias (`type ID = number`)
            // is encoded by the parser as a single field named
            // "__alias__"; skip the placeholder-sid reservation
            // and resolve to the underlying type instead.
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                // RFC 20260708-variadic chunk 1 — a variadic fn-type
                // alias has no SSA shape yet (the boxed_entry call
                // lane is chunk 2): skip registration so a declared-
                // but-unused alias stays inert while any USE keeps
                // the loud unknown-type reject.
                if fields[0].1.contains("__rest(") {
                    continue;
                }
                let ty = parse_type(
                    Some(fields[0].1.as_str()),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                );
                aliases.insert(name.clone(), ty);
                continue;
            }
            let sid = ssa::StructId(struct_layouts.len() as u32);
            struct_layouts.push(Vec::new());
            class_sids.insert(name.clone(), sid);
            aliases.insert(name.clone(), Type::Obj(sid));
        }
    }
    // Reserved sids whose layout has not been written yet — excluded
    // from the intern candidates below (an aliased class's slot stays
    // pending forever: it is empty and nothing references it).
    let mut pending_reserved: HashSet<u32> = class_sids.values().map(|s| s.0).collect();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() {
                generic_struct_decls.insert(name.clone(), (type_params.clone(), fields.clone()));
                continue;
            }
            // V3-18 wedge — already handled in the placeholder
            // pass above for bare aliases; skip here to avoid
            // accidentally finalizing a struct layout.
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                continue;
            }
            let mut layout: Vec<(PropKey, Type)> = Vec::with_capacity(fields.len());
            // W4 — class field widths join over all instances through
            // the nominal Class key. D5 — cyclic plain aliases take
            // the same nominal widths (their reserved sid closes the
            // recursion right here; see num_width/alias.rs). F3 —
            // generator `__step_*` aliases too, so the state machine's
            // value slot width joins over every yielded expression.
            // Other plain aliases widen per consuming slot instead.
            let class_key = (ast.class_parents.contains_key(name)
                || num_f64_slots.is_nominal_alias(name))
            .then(|| crate::num_width::SlotKey::Class(name.clone()));
            for (fname, fty_ann) in fields {
                let mut ty = parse_struct_field_type(
                    fty_ann.as_str(),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                );
                if let Some(ck) = &class_key {
                    let fkey = crate::num_width::SlotKey::Field(
                        Box::new(ck.clone()),
                        PropKey::from(fname),
                    );
                    ty = match ty {
                        Type::I64
                            if fty_ann == "number" && num_f64_slots.field_is_f64(ck, fname) =>
                        {
                            Type::F64
                        }
                        Type::Arr(_) => crate::ssa_lower_container_width::widen_arr_elem(
                            ty,
                            Some(fty_ann.as_str()),
                            &fkey,
                            num_f64_slots,
                            &mut arr_layouts,
                        ),
                        // F5 mirror of `widen_struct_fields` — an
                        // fn-typed field's sig widens by its own field
                        // key: the objlit constructor glue joins the
                        // literal's F5 `__ret`/`__p{i}` projections
                        // onto this Class field, so the `__mth(` parse
                        // width agrees with the resident method fn's
                        // analyzed ABI (twin-sid split otherwise).
                        Type::FnSig(_) | Type::Closure(_) => {
                            crate::ssa_lower_container_width::widen_fn_sig_by_key(
                                ty,
                                &fkey,
                                num_f64_slots,
                                &mut arr_layouts,
                                &mut fn_sigs,
                            )
                        }
                        other => other,
                    };
                }
                layout.push((PropKey::from(fname), ty));
            }
            intern_or_finalize(
                name,
                layout,
                class_sids[name],
                &mut struct_layouts,
                &mut aliases,
                &mut pending_reserved,
            );
        }
    }

    // Phase H.1.b — assign each declared class a runtime tag.
    // Tags are keyed by class name (not sid) because structurally
    // identical classes share a single sid via the intern table;
    // keying by sid would silently mis-route `__dispatch_<M>`. Tag 0
    // is reserved for "not a class". Walk names in lexical order so
    // codegen stays deterministic across builds (HashMap iteration
    // is unordered).
    let class_name_to_tag: HashMap<String, u32> = {
        let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
        class_names.sort();
        class_names
            .iter()
            .enumerate()
            .map(|(i, cname)| ((*cname).clone(), (i as u32) + 1))
            .collect()
    };

    Pass05 {
        aliases,
        arr_layouts,
        baked_regex_buf,
        fn_sigs,
        may_throw,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
        class_name_to_tag,
    }
}

/// Settle a declared class's freshly parsed layout: intern onto an
/// existing FINALIZED layout that matches (alias `name` to that sid and
/// leave the reserved slot empty — harmless, nothing references it), or
/// write it into the reserved slot.
///
/// A reserved slot that has not been finalized yet is an empty `Vec`
/// indistinguishable from a genuinely empty layout, so a zero-field
/// class declared ahead of any other class used to intern onto its
/// NEIGHBOUR's still-empty slot — which the neighbour then filled with
/// its own fields. `class A {}` + `class B { v = 7 }` gave `A`
/// instances B's layout: `new A()` printed `v: 0`, and the runtime drop
/// walked B's child offsets over A's smaller block (rotation 497 — the
/// iterator-helpers SIGBUS and the inspect `v: 0` drift behind the
/// injection-reachability gate were both this; the injected Error
/// family had merely kept user classes off the last-declared position
/// where the collision cannot happen). `pending_reserved` is the set
/// of reserved sids not yet written; an aliased class's slot stays in
/// it forever (empty, unreferenced) and never becomes a candidate.
fn intern_or_finalize(
    name: &str,
    layout: Vec<(PropKey, Type)>,
    reserved_sid: ssa::StructId,
    struct_layouts: &mut [Vec<(PropKey, Type)>],
    aliases: &mut HashMap<String, Type>,
    pending_reserved: &mut HashSet<u32>,
) {
    let found = struct_layouts.iter().enumerate().find_map(|(i, ex)| {
        let i = i as u32;
        (i != reserved_sid.0 && !pending_reserved.contains(&i) && *ex == layout)
            .then_some(ssa::StructId(i))
    });
    if let Some(canonical) = found {
        aliases.insert(name.to_string(), Type::Obj(canonical));
    } else {
        struct_layouts[reserved_sid.0 as usize] = layout;
        pending_reserved.remove(&reserved_sid.0);
    }
}
