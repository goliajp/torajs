//! The whole-program walk that computes a [`super::ShadowSet`] — how
//! the answer is found, kept apart from what the answer means.
//!
//! A child module so it writes the set's private fields and reads
//! `BUILTIN_CTORS` with no visibility changes: a descendant sees an
//! ancestor's private items. Split out in rotation 320 when the
//! `.constructor` reach pushed the single file past its limit.

use std::collections::{HashMap, HashSet};

use super::{BUILTIN_CTORS, Family, ShadowSet};
use crate::ast::{Ast, Expr, ExprId};

/// Walk the whole module once and answer what it might shadow.
///
/// Recomputed per function from the caller's own `Ast`, the same
/// shared-set contract as [`crate::dynobj_degrade`] and
/// [`crate::let_widen`]: both sides read one snapshot, so an `ExprId`
/// rewritten between check and lower cannot strand the result.
pub(crate) fn collect_shadowed_builtin_methods(ast: &Ast) -> ShadowSet {
    let mut set = ShadowSet::default();

    // Every `<expr>.prototype` occurrence, with the family it names.
    let mut proto_exprs: HashMap<ExprId, Family> = HashMap::new();
    // Reverse edge: base ExprId -> the accesses naming it as object.
    let mut bases: HashMap<ExprId, Vec<Access>> = HashMap::new();
    let mut assign_targets: HashSet<ExprId> = HashSet::new();
    let mut delete_targets: HashSet<ExprId> = HashSet::new();
    // `Object.defineProperty(P, "m", d)` — a write that reaches the
    // prototype without naming a member of it, so the base-use rule
    // above would read it as an escape. Attribute it instead.
    let mut define_calls: Vec<(ExprId, Option<String>)> = Vec::new();
    // Every mention of a builtin constructor BY NAME, and the subset
    // of expressions consumed in a position that only reads through
    // the name. A mention outside that subset means the constructor
    // VALUE went somewhere this scan cannot follow — `const A = Array`
    // being the plain case, `f(Array)` the other one.
    let mut ctor_idents: HashMap<ExprId, Family> = HashMap::new();
    let mut consumed: HashSet<ExprId> = HashSet::new();

    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Ident(name) => {
                if let Some(f) = BUILTIN_CTORS.iter().copied().find(|c| *c == name.as_str()) {
                    ctor_idents.insert(eid, f);
                }
            }
            Expr::Member { obj, name } => {
                if name == "prototype" {
                    if let Some(f) = builtin_named_by(ast, *obj) {
                        proto_exprs.insert(eid, f);
                    }
                }
                if name == "constructor" {
                    match literal_family(ast, *obj) {
                        // Spellable: this expression IS a mention of
                        // that constructor, so it joins the escape
                        // bookkeeping below on equal terms with a bare
                        // `Array`. Consumed by a `.prototype` read it
                        // attributes precisely; assigned to a variable
                        // it widens one family, exactly as
                        // `const A = Array` does.
                        Some(f) => {
                            ctor_idents.insert(eid, f);
                        }
                        None => set.all = true,
                    }
                }
                consumed.insert(*obj);
                bases
                    .entry(*obj)
                    .or_default()
                    .push(Access::Named(eid, name.clone()));
            }
            Expr::Index { obj, index } => {
                let key = str_literal(ast, *index);
                if key.as_deref() == Some("prototype")
                    && let Some(f) = builtin_named_by(ast, *obj)
                {
                    proto_exprs.insert(eid, f);
                }
                if key.as_deref() == Some("constructor") {
                    match literal_family(ast, *obj) {
                        Some(f) => {
                            ctor_idents.insert(eid, f);
                        }
                        None => set.all = true,
                    }
                }
                consumed.insert(*obj);
                bases.entry(*obj).or_default().push(match key {
                    Some(k) => Access::Named(eid, k),
                    None => Access::Computed(eid),
                });
            }
            // The remaining positions where naming a builtin
            // constructor does NOT hand its value to anyone: calling
            // it, reading through it, asking its type, casting it.
            Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. } => {
                consumed.insert(*obj);
            }
            Expr::OptCall { callee, .. } | Expr::NewDynamic { callee, .. } => {
                consumed.insert(*callee);
            }
            Expr::TypeOf { expr } | Expr::As { expr, .. } => {
                consumed.insert(*expr);
            }
            Expr::Assign { target, .. } => {
                assign_targets.insert(*target);
            }
            Expr::Delete { expr } => {
                delete_targets.insert(*expr);
            }
            Expr::Call { callee, args } => {
                consumed.insert(*callee);
                if let Some(kind) = reflective_callee(ast, *callee) {
                    match kind {
                        // Hands out a prototype object with no syntactic
                        // spelling of which one. `x.constructor` above
                        // is the same shape one step removed — it hands
                        // out the CONSTRUCTOR with no spelling of which
                        // one — and takes the same answer. Both could be
                        // narrowed by asking whether a write ever flows
                        // through the result; neither is, because the
                        // question is about a value this pass has
                        // already admitted it cannot follow, and a
                        // half-followed value is how the aliased-ctor
                        // hole (rotation 319) got in.
                        Reflective::GetProto => set.all = true,
                        Reflective::Define => {
                            if let Some(&target) = args.first() {
                                define_calls
                                    .push((target, args.get(1).and_then(|a| str_literal(ast, *a))));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // A builtin constructor whose name is mentioned anywhere other
    // than "read something through it" has escaped: `const A = Array`
    // makes `A.prototype.join = f` a patch this scan never sees,
    // because `A` is not a name it recognises. Today that lands the
    // program in L0 and the patch is IGNORED — silently, which is the
    // one outcome the design does not accept.
    //
    // Standing the whole family down is the sound answer and it is
    // cheap to be right about: the escaped name tells us WHICH family,
    // so this widens one family rather than reaching for `all`. Being
    // wrong here (a mention that could not have led to a patch) costs
    // that family its typed tier in that program — a slower program,
    // never a wrong one. `new Array(n)` and `x instanceof Array` are
    // not affected at all: both spell the constructor as a plain
    // String in the AST, not as an `Ident` expression.
    for (eid, family) in &ctor_idents {
        if !consumed.contains(eid) {
            set.widen(family);
        }
    }

    for (&proto, &family) in &proto_exprs {
        let mut attributed = false;

        for (target, key) in &define_calls {
            if *target == proto {
                attributed = true;
                match key {
                    Some(m) => {
                        set.methods.insert((family, m.clone()));
                    }
                    None => set.widen(family),
                }
            }
        }

        for access in bases.get(&proto).into_iter().flatten() {
            attributed = true;
            match access {
                // A read cannot install a patch; only a write or a
                // delete of this member can.
                Access::Named(member, name) => {
                    if assign_targets.contains(member) || delete_targets.contains(member) {
                        set.methods.insert((family, name.clone()));
                    }
                }
                Access::Computed(member) => {
                    if assign_targets.contains(member) || delete_targets.contains(member) {
                        set.widen(family);
                    }
                }
            }
        }

        // Parent is neither a member access nor a define call: the
        // prototype went somewhere this pass cannot follow.
        if !attributed {
            set.widen(family);
        }
    }

    set
}

/// One access naming some expression as its object.
enum Access {
    Named(ExprId, String),
    Computed(ExprId),
}

enum Reflective {
    GetProto,
    Define,
}

/// `Object.getPrototypeOf` / `Reflect.defineProperty` / siblings.
fn reflective_callee(ast: &Ast, callee: ExprId) -> Option<Reflective> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Object" && ns != "Reflect" {
        return None;
    }
    match name.as_str() {
        "getPrototypeOf" | "setPrototypeOf" => Some(Reflective::GetProto),
        "defineProperty" | "defineProperties" => Some(Reflective::Define),
        _ => None,
    }
}

/// The family a literal spells outright. `[]` is an Array with no
/// type information needed, so `[].constructor` names Array exactly
/// as surely as `Array` does — the one receiver shape where a
/// `.constructor` read stays attributable.
fn literal_family(ast: &Ast, e: ExprId) -> Option<Family> {
    Some(match ast.get_expr(e) {
        Expr::Array(_) => "Array",
        Expr::String(_) => "String",
        Expr::Number(_) => "Number",
        Expr::Bool(_) => "Boolean",
        Expr::BigInt { .. } => "BigInt",
        Expr::Regex { .. } => "RegExp",
        Expr::ObjectLit { .. } => "Object",
        _ => return None,
    })
}

/// Is this a `.constructor` read — `x.constructor` or
/// `x["constructor"]`? Answers the receiver expression, so the caller
/// can ask whether the family is spellable.
fn constructor_read_of(ast: &Ast, e: ExprId) -> Option<ExprId> {
    match ast.get_expr(e) {
        Expr::Member { obj, name } if name == "constructor" => Some(*obj),
        Expr::Index { obj, index }
            if str_literal(ast, *index).as_deref() == Some("constructor") =>
        {
            Some(*obj)
        }
        _ => None,
    }
}

/// The builtin constructor an expression names, if it names one
/// directly. Anything else — an alias, a user class, a call result —
/// answers `None`, which the caller reads as "unattributable family".
fn builtin_named_by(ast: &Ast, obj: ExprId) -> Option<Family> {
    if let Expr::Ident(name) = ast.get_expr(obj) {
        return BUILTIN_CTORS.iter().copied().find(|c| *c == name.as_str());
    }
    // `[].constructor.prototype.join = f` reaches the same object
    // `Array.prototype.join = f` does; spelling it here lets the
    // ordinary attribution below pin it to one family instead of
    // standing every family down.
    constructor_read_of(ast, obj).and_then(|recv| literal_family(ast, recv))
}

/// The string a literal key spells, for `p["join"]` shapes.
fn str_literal(ast: &Ast, e: ExprId) -> Option<String> {
    match ast.get_expr(e) {
        Expr::String(s) => Some(s.clone()),
        _ => None,
    }
}
