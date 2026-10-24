//! The scalars a cluster could keep in a register while it runs
//! (`PinFact::scalars`): a field of its receiver, or a module global,
//! that the members read and write as one integer word. The cluster is
//! already one of a sealed class whose members pass their own receiver
//! on, and nothing that runs while a member is on the stack runs user
//! code (F1–F3). A word is a scalar of the cluster when
//!
//! - S2 / G3, it is one integer word: the field's layout type, or the
//!   global's, is `i64`, `i32` or `bool`, and every access of it a
//!   member makes (for a global, every function that runs) has that
//!   type;
//! - S3, each member reaches the field only through its own receiver
//!   or a cell it allocated itself, never at a computed offset;
//! - S4 / G2, no function the members reach but the members themselves
//!   accesses the field (but on a cell it allocated) or names the
//!   global;
//! - S5, nothing the members reach calls a member from outside the
//!   cluster or takes a member's address: a member entered again would
//!   read the word from memory while the one below it holds it in the
//!   register;
//! - G1, the global's address goes nowhere but the base of a load or a
//!   store, in every function that runs.
//!
//! The members' static accesses rank the scalars; the first two are
//! kept. Only facts are written.

use std::collections::HashSet;

use crate::ssa::{FuncId, Function, InstKind, Module, Operand, PinScalar, StructId, Type};
use crate::ssa_lower::OBJ_HEADER_SIZE;

use super::roots::{Root, Roots};
use super::stats;

mod globals;
#[cfg(test)]
mod tests;

use globals::{global_access, global_refs, globals_held};

/// Scalars past this many stay in memory.
const MAX: usize = 2;

/// One word the members access, and why it cannot be a scalar.
struct Cand {
    scalar: PinScalar,
    name: String,
    uses: usize,
    no: Option<String>,
}

/// The scalars of `cluster`, a cluster of class sid `sid`; `skip`
/// names the functions that never run. Each word ruled out is printed
/// under `TORAJS_PIN_STATS` with its reason, after `label`.
pub(super) fn census(
    module: &Module,
    sid: StructId,
    cluster: &[FuncId],
    skip: &[bool],
    alloc: Option<FuncId>,
    label: &str,
) -> Vec<PinScalar> {
    let reach = reached(module, cluster);
    if let Some(why) = reentered(module, cluster, &reach) {
        if stats() {
            eprintln!("[pin] {label}: scalars: none: {why}");
        }
        return Vec::new();
    }
    let mut cands = members_words(module, sid, cluster, alloc);
    for fid in reach.iter().filter(|f| !cluster.contains(f)) {
        let f = &module.funcs[fid.0 as usize];
        outsider(f, sid, alloc, &mut cands);
    }
    globals_held(module, skip, &mut cands);
    let (mut kept, out): (Vec<Cand>, Vec<Cand>) = cands.into_iter().partition(|c| c.no.is_none());
    kept.sort_by_key(|c| std::cmp::Reverse(c.uses));
    if stats() {
        for c in &out {
            let why = c.no.as_deref().unwrap_or_default();
            eprintln!("[pin] {label}: scalar {}: none: {why}", c.name);
        }
        for c in kept.iter().skip(MAX) {
            eprintln!(
                "[pin] {label}: scalar {}: none: {MAX} more accessed",
                c.name
            );
        }
    }
    kept.truncate(MAX);
    kept.into_iter().map(|c| c.scalar).collect()
}

/// The display names of `scalars` of class sid `sid`.
pub(super) fn names(module: &Module, sid: StructId, scalars: &[PinScalar]) -> String {
    let name = |s: &PinScalar| match s {
        PinScalar::Field { off, .. } => field_name(module, sid, *off),
        PinScalar::Global { sym, .. } => sym.clone(),
    };
    scalars.iter().map(name).collect::<Vec<_>>().join(" ")
}

fn field_name(module: &Module, sid: StructId, off: u64) -> String {
    let idx = ((off - OBJ_HEADER_SIZE) / 8) as usize;
    module.struct_layouts[sid.0 as usize][idx]
        .0
        .to_string_lossy_owned()
}

/// The functions with a body the members reach by direct calls,
/// themselves included: the walk F2 makes.
fn reached(module: &Module, cluster: &[FuncId]) -> Vec<FuncId> {
    let mut seen: HashSet<FuncId> = cluster.iter().copied().collect();
    let mut out = cluster.to_vec();
    let mut i = 0;
    while let Some(&fid) = out.get(i) {
        i += 1;
        for inst in module.funcs[fid.0 as usize]
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
        {
            if let InstKind::Call(t, _) = &inst.kind
                && !module.funcs[t.0 as usize].is_declaration()
                && seen.insert(*t)
            {
                out.push(*t);
            }
        }
    }
    out
}

/// S5: the first function of `reach` that calls a member from outside
/// the cluster or takes a member's address.
fn reentered(module: &Module, cluster: &[FuncId], reach: &[FuncId]) -> Option<String> {
    reach.iter().find_map(|fid| {
        let f = &module.funcs[fid.0 as usize];
        let inside = cluster.contains(fid);
        f.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .find_map(|i| match &i.kind {
                InstKind::Call(t, _) if !inside && cluster.contains(t) => Some(format!(
                    "{} called by {}",
                    module.funcs[t.0 as usize].name, f.name
                )),
                InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) if cluster.contains(t) => {
                    Some(format!(
                        "{}'s address taken by {}",
                        module.funcs[t.0 as usize].name, f.name
                    ))
                }
                _ => None,
            })
    })
}

/// The type of `op` in `f`.
fn ty(f: &Function, op: &Operand) -> Type {
    match op {
        Operand::Value(v) => f.value_type(*v),
        Operand::ConstI64(_) => Type::I64,
        Operand::ConstI32(_) => Type::I32,
        Operand::ConstF64(_) => Type::F64,
        Operand::ConstBool(_) => Type::Bool,
        Operand::ConstPtrNull => Type::Ptr,
    }
}

fn word(t: Type) -> bool {
    matches!(t, Type::I64 | Type::I32 | Type::Bool)
}

/// A field access of `kind` in `f` on a cell of sid `sid`: the base,
/// the offset (`None` when computed) and the type moved.
fn field_access<'k>(
    f: &Function,
    sid: StructId,
    kind: &'k InstKind,
) -> Option<(&'k Operand, Option<u64>, Type)> {
    let cell = |p: &Operand| match ty(f, p) {
        Type::Obj(s) | Type::Inline(s) => s == sid,
        _ => false,
    };
    let (base, off, t) = match kind {
        InstKind::Load(t, p, off) => (p, Some(*off), *t),
        InstKind::Store(v, p, off) => (p, Some(*off), ty(f, v)),
        InstKind::LoadDyn(t, p, _) | InstKind::LoadDynScaled8(t, p, _) => (p, None, *t),
        InstKind::StoreDyn(v, p, _) | InstKind::StoreDynScaled8(v, p, _) => (p, None, ty(f, v)),
        InstKind::LoadU8Dyn(p, _) | InstKind::LoadU16Dyn(p, _) => (p, None, Type::I32),
        _ => return None,
    };
    let inline = matches!(ty(f, base), Type::Inline(_));
    cell(base).then_some((base, off.filter(|_| !inline), t))
}

/// The words the members access, with S2 and S3 answered for the
/// fields.
fn members_words(
    module: &Module,
    sid: StructId,
    cluster: &[FuncId],
    alloc: Option<FuncId>,
) -> Vec<Cand> {
    let layout = &module.struct_layouts[sid.0 as usize];
    let mut fields: Vec<Cand> = Vec::new();
    let mut globals: Vec<Cand> = Vec::new();
    let mut computed: Option<String> = None;
    for &m in cluster {
        let f = &module.funcs[m.0 as usize];
        let roots = Roots::of(f, alloc);
        let refs = global_refs(f);
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let Some((base, off, t)) = field_access(f, sid, &inst.kind) {
                let Some(off) = off else {
                    computed.get_or_insert_with(|| format!("a computed offset in {}", f.name));
                    continue;
                };
                let Some(idx) = off.checked_sub(OBJ_HEADER_SIZE).map(|o| (o / 8) as usize) else {
                    continue;
                };
                let field_ty = layout.get(idx).map_or(Type::Void, |(_, t)| *t);
                let c = cand(&mut fields, PinScalar::Field { off, ty: field_ty }, || {
                    field_name(module, sid, off)
                });
                c.uses += 1;
                let no = match roots.root(base) {
                    _ if !word(field_ty) => Some(format!("a {} field", field_ty.as_str())),
                    _ if t != field_ty => Some(format!("accessed as {} in {}", t.as_str(), f.name)),
                    Root::Receiver | Root::Fresh => None,
                    Root::Other => Some(format!("another cell's in {}", f.name)),
                };
                c.no = c.no.take().or(no);
                continue;
            }
            if let Some(sym) = global_access(&refs, &inst.kind) {
                let g = PinScalar::Global {
                    sym: sym.to_string(),
                    ty: Type::Void,
                };
                cand(&mut globals, g, || sym.to_string()).uses += 1;
            }
        }
    }
    if let Some(why) = computed {
        for c in &mut fields {
            c.no.get_or_insert_with(|| why.clone());
        }
    }
    fields.retain(|c| !matches!(&c.scalar, PinScalar::Field { off, .. } if is_arr(layout, *off)));
    fields.extend(globals);
    fields
}

fn is_arr(layout: &[(crate::ast::PropKey, Type)], off: u64) -> bool {
    let idx = ((off - OBJ_HEADER_SIZE) / 8) as usize;
    matches!(layout.get(idx), Some((_, Type::Arr(_))))
}

/// The candidate for `scalar` in `cands`, added when missing.
fn cand(cands: &mut Vec<Cand>, scalar: PinScalar, name: impl FnOnce() -> String) -> &mut Cand {
    let same = |c: &PinScalar| match (c, &scalar) {
        (PinScalar::Field { off: a, .. }, PinScalar::Field { off: b, .. }) => a == b,
        (PinScalar::Global { sym: a, .. }, PinScalar::Global { sym: b, .. }) => a == b,
        _ => false,
    };
    let at = match cands.iter().position(|c| same(&c.scalar)) {
        Some(i) => i,
        None => {
            cands.push(Cand {
                scalar,
                name: name(),
                uses: 0,
                no: None,
            });
            cands.len() - 1
        }
    };
    &mut cands[at]
}

/// S4 / G2 over `f`, a function the members reach.
fn outsider(f: &Function, sid: StructId, alloc: Option<FuncId>, cands: &mut [Cand]) {
    let roots = Roots::of(f, alloc);
    let named: HashSet<&str> = global_refs(f).into_values().collect();
    for c in cands.iter_mut() {
        let hit = match &c.scalar {
            PinScalar::Field { off, .. } => (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| {
                match field_access(f, sid, &i.kind) {
                    Some((base, o, _)) => {
                        o.is_none_or(|o| o == *off) && roots.root(base) != Root::Fresh
                    }
                    None => false,
                }
            }),
            PinScalar::Global { sym, .. } => named.contains(sym.as_str()),
        };
        if hit {
            c.no.get_or_insert_with(|| format!("accessed by {}", f.name));
        }
    }
}
