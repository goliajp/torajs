//! RFC 20260708 shared binding safety walk — split out of
//! `arguments_object_collect.rs` (file-size limit, rotation 362).
//! `safe_binding_chain` is the direct-call-or-alias binding walk
//! both the argc and argv value tiers gate on; `collect_legal_uses`
//! is its kill-walk legal-use set. The tier collectors stay in the
//! collect sibling.

use super::{Ast, Expr, ExprId, Stmt};

mod dup;

/// Rotation 362 nested tier — every `LetDecl` (name, init) in a
/// statement subtree, fn-nested bodies and all statement containers
/// included. The chain's direct/alias seeds walk this instead of the
/// top-level-only loop, which was the ONLY thing keeping fn-nested
/// `const f = function () { …arguments… }` bodies loud (the lifted
/// closure itself is already a top-level FnDecl the tier seeds see).
pub(super) fn collect_letdecls_recursive<'a>(body: &'a [Stmt], out: &mut Vec<(&'a str, ExprId)>) {
    for s in body {
        match s {
            Stmt::LetDecl { name, init, .. } => out.push((name.as_str(), *init)),
            Stmt::FnDecl { body, .. } => collect_letdecls_recursive(body, out),
            Stmt::ForOf { body, .. }
            | Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. } => {
                collect_letdecls_recursive(core::slice::from_ref(body), out)
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_letdecls_recursive(body, out);
                collect_letdecls_recursive(catch_body, out);
                if let Some(fb) = finally_body {
                    collect_letdecls_recursive(fb, out);
                }
            }
            Stmt::Block(inner) => collect_letdecls_recursive(inner, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_letdecls_recursive(core::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch {
                    collect_letdecls_recursive(core::slice::from_ref(eb), out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_letdecls_recursive(core::slice::from_ref(i), out);
                }
                collect_letdecls_recursive(core::slice::from_ref(body), out);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_letdecls_recursive(&c.body, out);
                }
                if let Some(d) = default {
                    collect_letdecls_recursive(d, out);
                }
            }
            _ => {}
        }
    }
}

/// Knife 4c (RFC 20260801-arguments-method-face) — `var ref =
/// obj.method`: a member read of a top-level object-literal binding
/// whose field holds a seeded closure joins the chain too (the
/// test262 meth-args template's escape form). Only the ALIAS binding
/// enters the walk — the object binding itself is untouched, and its
/// member CALLS stay safe because the argv face's rest-tail checker
/// type routes a reshaped field through the boxed variadic lane
/// rather than the static widen. The escape usually arrives
/// var-hoisted (`let ref: any (Uninit)` + a top-level `ref = obj.m`
/// Assign) — both spellings seed, and the Assign's own target Ident
/// is whitelisted for the kill walk (returned set): it is the
/// binding's definition, not a value use; any OTHER assignment to
/// the alias still kills the chain. Split from `safe_binding_chain`
/// (rotation-362 close audit — the dup-group wiring pushed it past
/// the 200-line fn limit).
fn seed_objlit_aliases(
    ast: &Ast,
    seed: &impl Fn(&str) -> bool,
    candidates: &mut std::collections::HashMap<String, String>,
) -> std::collections::HashSet<u32> {
    use std::collections::{HashMap, HashSet};
    let mut objlit_fields: HashMap<(&str, &str), &str> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::ObjectLit { fields } = ast.get_expr(*init)
        {
            for (fname, feid) in fields {
                if let Expr::Closure { fn_name, .. } = ast.get_expr(*feid) {
                    objlit_fields.insert((name.as_str(), fname.as_str()), fn_name.as_str());
                }
            }
        }
    }
    let mut assign_legal: HashSet<u32> = HashSet::new();
    for s in &ast.stmts {
        let (bname, member_eid, target_eid) = match s {
            Stmt::LetDecl { name, init, .. } => (name.clone(), *init, None),
            Stmt::Expr(eid) => {
                let Expr::Assign { target, value } = ast.get_expr(*eid) else {
                    continue;
                };
                let Expr::Ident(name) = ast.get_expr(*target) else {
                    continue;
                };
                (name.clone(), *value, Some(target.0))
            }
            _ => continue,
        };
        if let Expr::Member { obj, name: m } = ast.get_expr(member_eid)
            && let Expr::Ident(o) = ast.get_expr(*obj)
            && let Some(fn_name) = objlit_fields.get(&(o.as_str(), m.as_str()))
            && seed(fn_name)
            && !candidates.contains_key(&bname)
        {
            candidates.insert(bname, fn_name.to_string());
            if let Some(t) = target_eid {
                assign_legal.insert(t);
            }
        }
    }
    assign_legal
}

/// Shared binding safety walk (argc + argv tiers): seed direct
/// bindings (`const f = <closure literal>` whose fn passes `seed`),
/// grow aliases (`const g = f`) to a fixpoint, then kill every
/// chain containing a binding used anywhere other than a Call
/// callee or a still-candidate alias init. A killed chain removes
/// the whole source fn (its call sites don't know the reshaped
/// signature).
///
/// Rotation 362 — the direct and alias seeds walk fn-nested bodies
/// too. The kill walk resolves uses BY NAME over the whole arena, so
/// a name bound more than once anywhere (two same-named bindings in
/// different scopes) is dropped up front: the two chains would blur
/// into one — the conservative skip loses the optimization, never
/// admits wrongly.
pub(super) fn safe_binding_chain(ast: &Ast, seed: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    use std::collections::{HashMap, HashSet};
    let mut all_lets: Vec<(&str, ExprId)> = Vec::new();
    collect_letdecls_recursive(&ast.stmts, &mut all_lets);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut dup: HashSet<&str> = HashSet::new();
    for (n, _) in &all_lets {
        if !seen.insert(n) {
            dup.insert(n);
        }
    }
    let mut candidates: HashMap<String, String> = HashMap::new();
    for (name, init) in &all_lets {
        if !dup.contains(name)
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && seed(fn_name)
        {
            candidates.insert(name.to_string(), fn_name.clone());
        }
    }
    let assign_legal = seed_objlit_aliases(ast, &seed, &mut candidates);
    // Objlit / var-hoisted seeds stay top-level; the dup guard
    // still applies to whatever they inserted.
    candidates.retain(|b, _| !dup.contains(b.as_str()));
    loop {
        let mut grew = false;
        for (name, init) in &all_lets {
            if !dup.contains(name)
                && let Expr::Ident(src) = ast.get_expr(*init)
                && candidates.contains_key(src)
                && !candidates.contains_key(*name)
            {
                candidates.insert(name.to_string(), candidates[src].clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    // RFC 20260808 knife 2 — the kill walk's by-name arena scan must
    // not count a fn-local PARAM shadow's uses (the t262 harness
    // declares a `func` param whose body uses killed every binding
    // the bind family actually names `func`). Resolution is per fn —
    // `toplevel_binding_refs` cannot arbitrate here: its Closure arm
    // counts capture names unfiltered (the r290 over-approximation),
    // so the harness param captured by its own inner closure poisons
    // the whole set. A closure CAPTURE of the name outside a
    // shadowing body is a real reference the walk can't see through —
    // that still kills the chain.
    //
    // Knife 7 — the exemption arms ONLY for a binding that stands in
    // `.bind`-receiver position somewhere: those calls reach the
    // body through the bind kernel's boxed entry (real argc/argv),
    // the verified face. A shadow-colliding binding whose only uses
    // are DIRECT calls stays on the old kill: admitting it routes
    // the calls down the fn-indirect lane on the DECLARED signature
    // while the body grows the argc/argv slots — a 5-param body
    // entered through a 2-param sig read its argv param off garbage
    // (the 27 new SIGSEGVs of the every/filter/map `-c-i-25` t262
    // family). Real fix = the var-hoisted binding's rest-tail mint
    // (L3b); until then the un-exempted shape keeps its loud reject.
    let shadow_owned: std::collections::HashMap<String, Vec<bool>> = candidates
        .keys()
        .map(|b| {
            let has_bind_receiver_use = ast.exprs.iter().any(|e| {
                matches!(e, Expr::Member { obj, name } if name == "bind"
                    && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == b))
            });
            let owned = if has_bind_receiver_use {
                super::fnexpr_bind_this::shadowed_use_eids(ast, b)
            } else {
                vec![false; ast.exprs.len()]
            };
            (b.clone(), owned)
        })
        .collect();
    // RFC 20260808 escape-store profile — the face-position roots
    // the fnexpr-this store arm admits (B2, rotation 337): a store
    // into `<any>.k` / `<any>[k]` / `X.prototype.k` /
    // `<any|array>.constructor[k]` is boxed-only consumption — a
    // builtin reaches the stored value through the factory adapter
    // or the any-lane call path, both of which enter the boxed dual
    // entry with REAL argc/argv. Computed once for the kill walk's
    // escape-store exemption below.
    let props_recvs =
        super::fnexpr_this_recvs::collect_props_receiver_binding_names(&ast.stmts, &ast.exprs);
    let boxed_face_store = |target: ExprId| -> bool {
        super::arguments_object_escape_store::boxed_face_store_target(ast, target, &props_recvs)
    };
    // Rotation 345 — boxed-consumption ARGUMENT positions are legal
    // too, on the same reasoning as the escape-store profile: a value
    // handed to an explicit-`any` / proven-generic param crosses into
    // the any lane, and a construct-channel builtin argument reaches
    // its call through the construct kernel — both enter the boxed
    // dual entry with REAL argc/argv, never the declared static sig.
    let mut boxed_arg_sites = super::fnexpr_this_args::any_param_arg_idents(&ast.stmts, &ast.exprs);
    boxed_arg_sites.extend(super::fnexpr_this_args::construct_channel_arg_idents(
        &ast.exprs,
    ));
    // An equality operand (`r.constructor === C`) compares the cell
    // pointer — no call lane at all, so it cannot reach the body on
    // the declared signature either.
    boxed_arg_sites.extend(super::fnexpr_this_args::eq_operand_idents(&ast.exprs));
    // Rotation 362 — array-literal ELEMENT positions are boxed-only
    // consumption too (the container-store shape): the element slot
    // boxes the closure into the any world, and every call reached
    // back out of the array (`box[0](…)`, an aliased element read, a
    // spread) dispatches any-lane through the boxed dual entry with
    // REAL argc/argv — no path from inside an array reaches the
    // declared static signature.
    for e in &ast.exprs {
        if let Expr::Array(elems) = e {
            boxed_arg_sites.extend(elems.iter().copied());
        }
    }
    // Rotation 363 — the HOF callback slot (map/filter/forEach) is
    // boxed-only consumption on the downgraded channels: the array
    // inline loops, the Map/Set forEach walks and every non-array
    // receiver spelling (any-lane, struct method) all route an
    // argv-face callee through the boxed variadic dispatch with
    // REAL argc/argv, so a binding handed to that slot never
    // reaches the declared static signature.
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && let Expr::Member { name: m, .. } = ast.get_expr(*callee)
            && matches!(
                m.as_str(),
                "map" | "filter" | "forEach" | "reduce" | "reduceRight"
            )
            && let Some(&a0) = args.first()
        {
            boxed_arg_sites.insert(a0);
        }
    }
    loop {
        let mut killed: HashSet<String> = HashSet::new();
        for b in candidates.keys() {
            let sh = &shadow_owned[b];
            if ast.exprs.iter().enumerate().any(|(i, e)| {
                matches!(e, Expr::Closure { captures, .. }
                    if captures.iter().any(|c| c == b))
                    && !sh[i]
            }) {
                killed.insert(b.clone());
                continue;
            }
            let b_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
                .filter(|&i| {
                    matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == b) && !sh[i as usize]
                })
                .collect();
            let legal = collect_legal_uses(ast, &candidates, &b_eids, &assign_legal, |t| {
                boxed_face_store(t)
            });
            if b_eids
                .difference(&legal)
                .any(|&i| !boxed_arg_sites.contains(&ExprId(i)))
            {
                killed.insert(b.clone());
            }
        }
        if killed.is_empty() {
            break;
        }
        let killed_fns: HashSet<String> = candidates
            .iter()
            .filter(|(b, _)| killed.contains(*b))
            .map(|(_, f)| f.clone())
            .collect();
        candidates.retain(|_, f| !killed_fns.contains(f));
    }
    let mut pairs: Vec<(String, String)> = candidates.into_iter().collect();
    pairs.extend(dup::admit_dup_groups(
        ast,
        &dup,
        &seed,
        &assign_legal,
        &boxed_face_store,
        &boxed_arg_sites,
    ));
    pairs
}

/// The kill walk's legal-use set for one candidate binding — every
/// arena occurrence of the name that does NOT kill the chain:
/// * a Call callee (the direct-site shape the fold rewrites);
/// * a still-candidate alias init (`const g = f`);
/// * the knife-4c seeding Assign's own target Ident (the binding's
///   definition, not a value use);
/// * RFC 20260808 escape-store — a store into a boxed-face position:
///   every path from that slot back to the body (species construct,
///   builtin callback, any-lane member call) enters the boxed dual
///   entry, which feeds the reshaped signature its REAL argc/argv;
/// * RFC 20260808 knife 2 — `b.bind(…)` as a Call callee: the bind
///   kernel's bound cell dispatches through the boxed dual entry. A
///   `.bind` member read outside a call (a stored `b.bind` value)
///   still kills;
/// * species key 2 — `b.prototype` as a member-read receiver, ONLY
///   for a binding that also has an escape-store use: the read
///   touches the closure cell's fnprops canonical pair (RFC
///   20260804), never the body, and a prototype-object round trip
///   back to the fn value (`b.prototype.constructor`) dispatches
///   any-lane through the boxed dual entry — real argc/argv again.
///   (create-species.js asserts `Object.getPrototypeOf(thisValue)
///   === Ctor.prototype` — the read was killing the whole chain.)
///   The store gate is load-bearing: a store-free fn-expr ctor
///   (`var F = function () {…arguments…}; new F(…)`) rides the
///   ctor-argv STATIC channel, which the kill of its desugar-
///   synthesized `F.prototype` read is what selects — exempting it
///   unconditionally floated those bindings into the boxed argv
///   face against static call sites (the fnexpr-ctor-args-001
///   TypeError/SIGSEGV pair, gate 8692ca1a').
///
/// Any other use shape kills.
pub(super) fn collect_legal_uses(
    ast: &Ast,
    candidates: &std::collections::HashMap<String, String>,
    b_eids: &std::collections::HashSet<u32>,
    assign_legal: &std::collections::HashSet<u32>,
    boxed_face_store: impl Fn(ExprId) -> bool,
) -> std::collections::HashSet<u32> {
    let mut legal: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e
            && b_eids.contains(&callee.0)
        {
            legal.insert(callee.0);
        }
    }
    // Rotation 362 — the alias-init exemption walks nested bodies
    // like the seeds do (a fn-nested `const g = f` init is the
    // binding's definition, not a value use).
    let mut all_lets: Vec<(&str, ExprId)> = Vec::new();
    collect_letdecls_recursive(&ast.stmts, &mut all_lets);
    for (name, init) in &all_lets {
        if b_eids.contains(&init.0) && candidates.contains_key(*name) {
            legal.insert(init.0);
        }
    }
    for &t in assign_legal.intersection(b_eids) {
        legal.insert(t);
    }
    let mut escape_stored = false;
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && b_eids.contains(&value.0)
            && boxed_face_store(*target)
        {
            legal.insert(value.0);
            escape_stored = true;
        }
    }
    for (mi, e) in ast.exprs.iter().enumerate() {
        if let Expr::Member { obj, name } = e
            && b_eids.contains(&obj.0)
            && ((name == "prototype" && escape_stored)
                || (name == "bind"
                    && ast
                        .exprs
                        .iter()
                        .any(|c| matches!(c, Expr::Call { callee, .. } if callee.0 == mi as u32))))
        {
            legal.insert(obj.0);
        }
    }
    legal
}
