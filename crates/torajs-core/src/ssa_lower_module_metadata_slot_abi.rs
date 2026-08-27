//! 507-02 — is the vtable actually callable?
//!
//! `populate_vtables` in the parent module builds the table; this
//! answers the one question the emitter needs of it. Split out rather
//! than added there because that file was already 437 lines and this
//! is a different question (build vs check), with its own two helpers.

use std::collections::HashMap;

use crate::ssa::{self, Module};

/// 507-02 — one vtable slot, one ABI, per hierarchy. A
/// `CallIndirect` through slot `i` is emitted with the signature of a
/// SINGLE body — the base declarer at the call site — so if two rows
/// a call site can reach put bodies of different machine shapes in
/// that slot, one of them is entered under the wrong ABI: silently,
/// with the return value read back as whatever bits land in the
/// register the caller reads (the shape rotation 507 measured as
/// `-this.id` printing 4380426256).
///
/// "A call site can reach" is per hierarchy ROOT, not global: a
/// receiver can only ever wear a row from its own chain, so an
/// unrelated class that happens to declare the same method name fills
/// the shared slot with its own body under its own signature and no
/// site sees both. That is exactly the population `num_width::slot_abi`
/// unions, and this checks the table those unions were meant to make
/// callable — the only place that pass's idea of a slot and the
/// emitter's have to agree. Measured: silent on the whole case corpus,
/// and with `slot_unions` disabled it names the rotation-507 pair
/// (`__cm_Base__score` I64 vs `__cm_Other__score` F64).
///
/// The comparison is the machine shape (float / word / void per
/// position), not the SSA type: a receiver is `Obj(<own class>)` in
/// every row by construction, and two word-sized types in one slot
/// call the same way. A pair that agrees on shape but not on type is
/// a narrower question — see 507-06 — and is deliberately not fatal
/// here.
///
/// Loud rather than a degrade: a slot whose rows disagree has no
/// correct lowering, so the honest answer is to stop.
pub(crate) fn assert_vtable_slot_abi(
    ast: &crate::ast::Ast,
    module: &Module,
    fn_sig_ids: &HashMap<ssa::FuncId, ssa::SigId>,
) {
    let mut claimed: HashMap<(usize, String), (String, String)> = HashMap::new();
    for row in &module.vtable_globals {
        let root = hierarchy_root(ast, &row.class_name);
        for (slot, fid) in row.fn_ids.iter().enumerate() {
            let Some(fid) = fid else { continue };
            // A body with no interned signature is not reachable
            // through the indirect lane either: the call site's own
            // `fn_sig_ids` miss makes it fall back to a direct call.
            let Some(sig) = fn_sig_ids.get(fid).copied() else {
                continue;
            };
            let (params, ret) = &module.signatures[sig.0 as usize];
            let shape: String = params.iter().chain([ret]).map(abi_class).collect();
            let name = &module.funcs[fid.0 as usize].name;
            match claimed.get(&(slot, root.clone())) {
                Some((first_shape, first_name)) if *first_shape != shape => panic!(
                    "ssa-lower: vtable slot {slot} of hierarchy `{root}` holds two ABIs — \
                     `{first_name}` is {first_shape}, `{name}` is {shape}; one slot, one shape"
                ),
                Some(_) => {}
                None => {
                    claimed.insert((slot, root.clone()), (shape, name.clone()));
                }
            }
        }
    }
}

/// How a value of this type is passed: in a float register, in a
/// general register, or not at all.
fn abi_class(t: &ssa::Type) -> char {
    match t {
        ssa::Type::F64 => 'f',
        ssa::Type::Void => 'v',
        _ => 'w',
    }
}

/// Topmost ancestor of `c` along `class_parents`, with a mono row's
/// `$$<suffix>` tail dropped first (a specialization sits in its base
/// class's chain). Hop-bounded against a malformed `extends` cycle.
fn hierarchy_root(ast: &crate::ast::Ast, c: &str) -> String {
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
