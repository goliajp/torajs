//! Parent-chain accessor lookup — rotation 413 blade 2, generic
//! substitution added in blade 3.
//!
//! `ast.accessor_getters` / `ast.accessor_setters` record each pair
//! under the DECLARING class only, so `class B extends A {}` probing
//! `("B", "x")` missed an accessor A declared and fell through to the
//! any-member lane: the read still answered right at runtime (the
//! §10.1.7 prototype walk hits the reified AccessorPair) but the
//! checker typed it `Any` and the lowering paid a dynamic dispatch.
//! These walks resolve the pair the way [[Get]]/[[Set]] would — own
//! class first, then up `class_parents` — so an inherited accessor
//! keeps its declared type and its direct call.
//!
//! The walk starts from a class KEY that may be a generic
//! instantiation (`"Box<number>"`) and composes the written heritage
//! arguments per hop (`class_parent_type_args` × `class_type_params`,
//! the `compute_full_fields` flattening recipe), so a hit on a
//! generic declarer carries the substitution that maps its type-param
//! names to spellings resolvable at the receiver. Hop-capped by the
//! class count (heritage cycles are rejected upstream; the cap keeps
//! a malformed table finite).

use std::collections::HashMap;

use super::{Ast, PropKey};
use crate::check_type_ann::split_top_pipe;
use crate::check_type_ann_substitute::ann_substitute;

pub(crate) struct AccessorHit {
    pub fn_name: String,
    /// The declaring class's type-param names zipped against the
    /// instantiation-argument spellings composed down the walk —
    /// empty for a non-generic declarer.
    pub subst: Vec<(String, String)>,
}

/// The lifted function names of OBJECT-LITERAL getters for `prop`.
///
/// A class accessor is recorded in `ast.accessor_getters` under its
/// declaring class, which is what the walks below resolve. An object
/// literal has no class to record under: the parser rewrites `get x()
/// {…}` into a `__getter_x` field holding an arrow, and the lift
/// turns that arrow into a `Closure { fn_name }`. So the only place
/// the connection survives is the literal itself, and the only way to
/// ask is by property name.
///
/// By name and across every literal in the program, like the sentinel
/// tables that call this: two literals with a `g` getter answer both
/// names, and a caller that is asking "can a read of `.g` hand back
/// the sentinel" wants exactly that union.
pub(crate) fn objlit_getter_fns<'a>(ast: &'a Ast, prop: &str) -> Vec<&'a str> {
    let slot = torajs_wtf8::Wtf8Buf::from(format!("__getter_{prop}"));
    let mut out = Vec::new();
    for id in 0..ast.exprs.len() {
        let super::Expr::ObjectLit { fields } = ast.get_expr(super::ExprId(id as u32)) else {
            continue;
        };
        for (fname, value) in fields {
            if fname.as_wtf8() != slot.as_ref() {
                continue;
            }
            if let super::Expr::Closure { fn_name, .. } = ast.get_expr(*value) {
                out.push(fn_name.as_str());
            }
        }
    }
    out
}

/// Every function a read of property `prop` can run as a getter:
/// instance accessors, static accessors, and object-literal getters.
///
/// The three live in three tables keyed three different ways — the
/// first two by DECLARING CLASS, the third only by the literal's own
/// `__getter_<prop>` field — so asking "which function does reading
/// `.g` run" means asking all three and taking the union. By name
/// and program-wide, which is the same trade the sentinel tables
/// already make: two classes with a `g` accessor answer both, and a
/// caller weighing "can this read hand back the sentinel" wants the
/// union, because missing one is silent and including one costs a
/// predictable compare.
///
/// The caller supplies its own "and can that function hand one back"
/// test, because the two callers ask it of different objects at
/// different times: the width analysis asks the sentinel table it is
/// still closing, lowering asks the finished width table. The
/// name -> function hop is the half that must not be spelled twice.
pub(crate) fn getter_fn_names<'a>(ast: &'a Ast, prop: &str) -> Vec<&'a str> {
    let key = torajs_wtf8::Wtf8::new(prop);
    let mut out: Vec<&'a str> = ast
        .accessor_getters
        .iter()
        .chain(ast.static_accessor_getters.iter())
        .filter(|((_, p), _)| p.as_wtf8() == key)
        .map(|(_, f)| f.as_str())
        .collect();
    out.extend(objlit_getter_fns(ast, prop));
    out
}

/// Every getter by the property name it serves, in one arena pass:
/// [`getter_fn_names`] for all names at once.
pub(crate) fn getter_fns_by_prop(ast: &Ast) -> HashMap<String, Vec<&str>> {
    let mut out: HashMap<String, Vec<&str>> = HashMap::new();
    let class = ast.accessor_getters.iter();
    for ((_, p), f) in class.chain(ast.static_accessor_getters.iter()) {
        let prop = p.as_wtf8().to_string_lossy().into_owned();
        out.entry(prop).or_default().push(f.as_str());
    }
    for id in 0..ast.exprs.len() {
        let super::Expr::ObjectLit { fields } = ast.get_expr(super::ExprId(id as u32)) else {
            continue;
        };
        for (fname, value) in fields {
            let name = fname.as_wtf8().to_string_lossy();
            if let Some(prop) = name.strip_prefix("__getter_")
                && let super::Expr::Closure { fn_name, .. } = ast.get_expr(*value)
            {
                out.entry(prop.to_string())
                    .or_default()
                    .push(fn_name.as_str());
            }
        }
    }
    out
}

/// The getter a read of `prop` through a receiver statically typed
/// `cls_key` runs, when one function or one vtable slot answers for
/// every instance that type admits. `None` sends the site to the
/// any-member lane, whose runtime walk settles it: no accessor at all,
/// a pair that lacks the getter (§10.1.8.1 answers undefined), or a
/// subclass shadowing the pair in a way no slot carries
/// ([`face_varies_below`]).
pub(crate) fn accessor_getter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    let faces = Faces {
        this: &ast.accessor_getters,
        other: &ast.accessor_setters,
        suffix: "_get",
    };
    accessor_in_chain(&faces, ast, cls_key, prop)
}

/// The setter twin of [`accessor_getter_in_chain`]; a pair that lacks
/// the setter refuses the write at runtime (§10.1.9.2 step 7) in the
/// writer's strictness.
pub(crate) fn accessor_setter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    let faces = Faces {
        this: &ast.accessor_setters,
        other: &ast.accessor_getters,
        suffix: "_set",
    };
    accessor_in_chain(&faces, ast, cls_key, prop)
}

/// One face of the accessor tables, read alongside the other. A class
/// declaring either face owns the whole property (one accessor
/// property per key on its prototype), so the walk stops there either
/// way.
struct Faces<'a> {
    this: &'a HashMap<(String, PropKey), String>,
    other: &'a HashMap<(String, PropKey), String>,
    suffix: &'static str,
}

/// Whether a strict descendant of `head` redeclares `prop` in a way
/// this face's vtable slot does not carry: the descendant's pair lacks
/// the face, or the face holds no slot listing it (a regular method
/// spelled like the face's symbol keeps the accessor out of the
/// slots). The function a read or write runs then depends on the
/// instance in a way only the runtime walk knows.
fn face_varies_below(faces: &Faces<'_>, ast: &Ast, head: &str, prop: &str) -> bool {
    let key = PropKey::from(prop);
    let slot = format!("{}{}", super::mangle_key(&key), faces.suffix);
    let owners = ast
        .method_index
        .contains_key(&slot)
        .then(|| ast.method_owners.get(&slot))
        .flatten();
    faces
        .this
        .keys()
        .chain(faces.other.keys())
        .filter(|(c, p)| *p == key && c != head)
        .filter(|(c, _)| super::method_owner_is_in_chain(&ast.class_parents, head, c))
        .any(|(c, _)| {
            !faces.this.contains_key(&(c.clone(), key.clone()))
                || !owners.is_some_and(|o| o.contains(c))
        })
}

fn accessor_in_chain(
    faces: &Faces<'_>,
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    // A generic-instantiation key seeds the substitution from its own
    // written arguments; a bare class name starts empty.
    let (head, mut subst): (&str, Vec<(String, String)>) = match cls_key.find('<') {
        Some(i) if cls_key.ends_with('>') => {
            let head = &cls_key[..i];
            let args = split_top_pipe(&cls_key[i + 1..cls_key.len() - 1]);
            let tps = ast.class_type_params.get(head)?;
            (
                head,
                tps.iter()
                    .enumerate()
                    .map(|(k, tp)| {
                        (
                            tp.clone(),
                            args.get(k)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "any".to_string()),
                        )
                    })
                    .collect(),
            )
        }
        _ => (cls_key, Vec::new()),
    };
    if face_varies_below(faces, ast, head, prop) {
        return None;
    }
    let mut cur = Some(head.to_string());
    for _ in 0..=ast.class_parents.len() {
        let c = cur?;
        let key = (c.clone(), PropKey::from(prop));
        if let Some(f) = faces.this.get(&key) {
            return Some(AccessorHit {
                fn_name: f.clone(),
                subst,
            });
        }
        if faces.other.contains_key(&key) {
            return None;
        }
        // Compose the substitution for the parent's vocabulary before
        // hopping — the written heritage args are spelled in `c`'s
        // vocabulary, so the current subst applies to them first
        // (chains re-spelling params stay concrete at the receiver).
        let parent = ast.class_parents.get(&c).cloned().flatten();
        if let Some(p) = &parent {
            let ptp = ast.class_type_params.get(p).cloned().unwrap_or_default();
            subst = if ptp.is_empty() {
                Vec::new()
            } else {
                let written = ast.class_parent_type_args.get(&c);
                ptp.iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let raw = written
                            .and_then(|a| a.get(i))
                            .cloned()
                            .unwrap_or_else(|| "any".to_string());
                        let arg = if subst.is_empty() {
                            raw
                        } else {
                            ann_substitute(&raw, &subst)
                        };
                        (tp.clone(), arg)
                    })
                    .collect()
            };
        }
        cur = parent;
    }
    None
}
