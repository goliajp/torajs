//! Pass 1 of `lower_inner` — pre-allocate FuncIds + record correct
//! return types for every user FnDecl, with Pass 0.4 (register every
//! Pass-0 intrinsic's signature in `fn_sig_ids`) folded in. The
//! placeholder body is empty; Pass 2 fills it in. Setting the right
//! ret type up front lets callsites resolve `f_ret_type_hint` even
//! before the callee's body has been lowered (mutual recursion,
//! forward refs, return-type-bool functions).
//!
//! W1 / W4 widening is applied to both param and return types so that
//! call sites coerce I64 / F64 / container-elem boundaries correctly
//! (see `ssa_lower::lower_inner` doc + num_width module).
//!
//! Extracted from `lower_inner` to drain the ~144-LOC Pass 1 region
//! out of the god-fn (chunk-329 of the lower_inner RFC decomp, after
//! Pass 0.5 sibling in chunk-328). Pure mechanical move: substrate
//! codegen is invariant (binary byte-identical with chunk-328).

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, FuncId, Module, Type};
use crate::ssa_lower::{effective_ret_ty, intern_fn_sig};
use crate::ssa_lower_parse_type::parse_type;

/// RFC 20260810-indirect-argc-abi S1 — where the hidden I64
/// `__torajs_argc` sits in this fn's SSA signature, on every tier
/// that has one: right after `__env` for a lifted closure entry
/// (S3.2), right after the head for a this-first body (S1-T1 and
/// rotation 616 / 627 / 650, see [`this_first_hidden_argc_pos`]),
/// at position 0 for a head-less T-31 body (H1 — no head to sit
/// behind). `None` for a fn with no hidden slot. Sig side (pass 1),
/// def side (`setup_fn_params`), the literal-default mirror and the
/// boxed adapter's skip math all key on this one predicate — keeping
/// it in one place is what stops the sites drifting (they were three
/// spellings of the same `if`/`else if` chain until rotation 650).
pub(crate) fn hidden_argc_pos(
    ast: &Ast,
    name: &str,
    params: &[crate::ast::Param],
) -> Option<usize> {
    if params.first().is_some_and(|p| p.name == "__env") {
        return Some(1);
    }
    if let Some(pos) = this_first_hidden_argc_pos(ast, name, params) {
        return Some(pos);
    }
    ast.headless_argc_fns.contains(name).then_some(0)
}

/// Where the hidden I64 `__torajs_argc` sits in a this-first sig —
/// right after the head, and a head is as wide as the declaration
/// says. A method carries one slot (`__this`); a constructor carries
/// two (`__this`, `__new_target`), so its argc lands at 2 (rotation
/// 616).
///
/// Two families carry the slot. The direct-call this-first tiers
/// (ctors, static twins, promoted `this`-reading declarations) are
/// keyed by NAME in `Ast::this_first_argc_fns`, whose value is the
/// head width — since rotation 650 a length-only body on that face
/// has no `__torajs_argv` to be recognised by, so the param list
/// cannot mark it. The `__cm_` method_argv family (S1-T1: bodies
/// the arguments pass admitted off the dispatch / vtable lanes) is
/// marked by the `__torajs_argv` `inject_argc_params` put right
/// after `__this`.
pub(crate) fn this_first_hidden_argc_pos(
    ast: &Ast,
    name: &str,
    params: &[crate::ast::Param],
) -> Option<usize> {
    if params.first().is_none_or(|p| p.name != "__this") {
        return None;
    }
    if let Some(&head) = ast.this_first_argc_fns.get(name) {
        return Some(head);
    }
    params
        .get(1)
        .is_some_and(|p| p.name == "__torajs_argv")
        .then_some(1)
}

/// S3.8 — a typed param's literal default (Number / Bool / short
/// string), positionally aligned with the fn's sig `param_tys` (the
/// `__env`-first hidden argc slot mirrors as a `None`). The
/// direct-call terminal consults this to bind the default when a
/// runtime `undefined` arrives in an `any` actual (§10.2.11 — the
/// call-site pad only covers the static spellings). Only fns with at
/// least one qualifying literal enter the map.
fn dflt_lits_of_params(
    ast: &Ast,
    name: &str,
    params: &[crate::ast::Param],
) -> Option<Vec<Option<crate::ssa_lower_boxed_entry::DfltLit>>> {
    use crate::ssa_lower_boxed_entry::DfltLit;
    let mut lits: Vec<Option<DfltLit>> = params
        .iter()
        .map(|p| {
            p.default
                .and_then(|d| DfltLit::of_default_expr(ast.get_expr(d)))
        })
        .collect();
    if !lits.iter().any(Option::is_some) {
        return None;
    }
    if let Some(pos) = hidden_argc_pos(ast, name, params) {
        lits.insert(pos, None);
    }
    Some(lits)
}

pub(crate) struct Pass1 {
    pub decl_indices: Vec<(usize, FuncId)>,
    pub fn_sig_ids: HashMap<FuncId, ssa::SigId>,
    /// S3.8 — per-fn Number/Bool literal defaults, sig-aligned (see
    /// [`dflt_lits_of_params`]).
    pub fn_dflt_lits: HashMap<FuncId, Vec<Option<crate::ssa_lower_boxed_entry::DfltLit>>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    num_f64_slots: &WidthTable,
    generic_fn_names: &HashSet<String>,
) -> Pass1 {
    let mut decl_indices: Vec<(usize, FuncId)> = Vec::new();
    let mut fn_sig_ids: HashMap<FuncId, ssa::SigId> = HashMap::new();
    let mut fn_dflt_lits: HashMap<FuncId, Vec<Option<crate::ssa_lower_boxed_entry::DfltLit>>> =
        HashMap::new();

    crate::ssa_lower_intrinsics_any_cast::declare_on_demand(module, fn_table, ast);
    crate::ssa_lower_index::dict::declare(module, fn_table, ast);
    // Pass 0.4 — register every Pass-0 intrinsic's signature in
    // `fn_sig_ids`. The call-site coercion arm later (`Expr::Call`
    // lowering, F64↔I64 directions) needs the param type list to
    // decide whether to insert SiToFp / FpToSi at boundary, and it
    // looks up the sig via `fn_sig_ids`. Without this, intrinsic
    // calls like `Math.imul(0.1, 7)` skip coercion and trip LLVM's
    // verifier with "Call parameter type does not match function
    // signature." Walks every Func currently in `module.funcs` —
    // since this runs before the user-decl pass, the only entries
    // are the Pass-0 intrinsics declared above.
    for (idx, f) in module.funcs.iter().enumerate() {
        let fid = FuncId(idx as u32);
        let param_tys: Vec<Type> = f.params.iter().map(|p| f.values[p.0 as usize].ty).collect();
        let sig = intern_fn_sig(fn_sigs, param_tys, f.ret);
        fn_sig_ids.insert(fid, sig);
    }
    for (i, stmt) in ast.stmts.iter().enumerate() {
        if let Stmt::FnDecl {
            name,
            return_type,
            params,
            type_params,
            body,
            ..
        } = stmt
        {
            // M3 — skip generic FnDecls. Their TypeVar-bearing annotations
            // (`T`, `T[]`, etc.) can't be parsed by `parse_type`, and the
            // monomorphization pre-pass has already produced concrete
            // specializations that the regular pass picks up below.
            if !type_params.is_empty() || generic_fn_names.contains(name) {
                continue;
            }
            let mut param_tys = Vec::with_capacity(params.len());
            for p in params {
                let mut pty = parse_type(
                    p.type_ann.as_deref(),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                );
                // W1 (ann-width RFC) — `: number` parses to the I64
                // default; the module-wide width inference decides
                // whether any statically-possible f64 value reaches
                // this param (body assignment, call-site arg, slot
                // propagation). Widening here makes call sites coerce
                // i64 args via SiToFp. Same num_width ground truth as
                // the body-lowering param_setup — the two sites must
                // not drift (K.3 lesson). Synthetic fns (`__cm_*` /
                // `__closure_*`) consult the same table (§5.6 F2):
                // their call sites read the same Pass-1 sig / Pass-2
                // signatures entries this widen feeds, so both ends
                // move together.
                if pty == Type::I64
                    && p.type_ann.as_deref() == Some("number")
                    && num_f64_slots.slot_is_f64(&crate::num_width::SlotKey::Param(
                        name.clone(),
                        p.name.clone(),
                    ))
                {
                    pty = Type::F64;
                }
                // W4 — container elem widths come from the alias-class
                // table. No `__` exclusion: Arr is pointer-shaped at
                // the ABI, and the arr_id must agree across the fn
                // boundary with the caller's value.
                pty = crate::ssa_lower_container_width::widen_container_ty(
                    pty,
                    p.type_ann.as_deref(),
                    &crate::num_width::SlotKey::Param(name.clone(), p.name.clone()),
                    num_f64_slots,
                    arr_layouts,
                    struct_layouts,
                    fn_sigs,
                );
                // the replace lane's view twin takes Substr views where
                // the original takes strings (`setup_fn_params` twin)
                if pty == Type::Str && ast.is_replace_view_fn(name) {
                    pty = Type::Substr;
                }
                param_tys.push(pty);
            }
            // RFC 20260810-indirect-argc-abi S1 — every `__env`-first
            // fn (lifted closure entry) carries a hidden I64
            // `__torajs_argc` at sig position 1, SSA-only (the AST
            // param list and the checker's Function face never see
            // it). S1-T1 extends the slot to the this-first
            // families (see [`this_first_hidden_argc_pos`]) — same
            // position, right after the head param.
            // `setup_fn_params` injects the def-side twin; the two
            // sites must not drift (same contract as the W1 widen
            // note above). H1 — the head-less tier has no head
            // param, so "right after the head" degenerates to sig
            // position 0; the direct-call terminal prepends the
            // argc operand.
            if let Some(pos) = hidden_argc_pos(ast, name, params) {
                param_tys.insert(pos, Type::I64);
            }
            let mut ret_ty = effective_ret_ty(
                parse_type(
                    return_type.as_deref(),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                ),
                ast,
                params,
                body,
            );
            // W1 — `(): number` ret slot widens when any return
            // expression is f64-possible; without this the return
            // site narrowed f64 results through FpToSi (R1: `return
            // 0.5` printed 0, silent wrong).
            if ret_ty == Type::I64
                && return_type.as_deref() == Some("number")
                && num_f64_slots.slot_is_f64(&crate::num_width::SlotKey::Ret(name.clone()))
            {
                ret_ty = Type::F64;
            }
            ret_ty = crate::ssa_lower_container_width::widen_container_ty(
                ret_ty,
                return_type.as_deref(),
                &crate::num_width::SlotKey::Ret(name.clone()),
                num_f64_slots,
                arr_layouts,
                struct_layouts,
                fn_sigs,
            );
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(name.clone(), fid);
            // Intern this user fn's signature — needed for `let f = name`
            // (allocate FnSig slot of the right type) and for emitting
            // FnAddr's result type. M2 Phase B Stage 4.
            let sig_id = intern_fn_sig(fn_sigs, param_tys, ret_ty);
            fn_sig_ids.insert(fid, sig_id);
            if let Some(lits) = dflt_lits_of_params(ast, name, params) {
                fn_dflt_lits.insert(fid, lits);
            }
            module.funcs.push(ssa::Function::new(name.clone(), ret_ty));
            decl_indices.push((i, fid));
        }
    }

    Pass1 {
        decl_indices,
        fn_sig_ids,
        fn_dflt_lits,
    }
}
