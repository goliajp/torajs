//! The function values held only by fn-typed slots of the boxed-entry
//! census, whose calls answer what the stored function returns.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check_type_of_call::closure_args;

/// A slot that holds what it is handed only when the census of a round
/// boxes it: a binding under a function-type annotation a store reaches
/// (by name), or a fn-typed parameter of a closure (function, name).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Guarded {
    Let(String),
    Param(String, String),
}

/// What [`fn_slot_values`] finds.
pub(super) struct SlotFlow {
    /// The reads of a function or slot that hand it to a slot.
    pub(super) values: HashSet<ExprId>,
    /// (name read, slot it is handed to, the census entry the slot
    /// needs), for each read in `values`.
    hands: Vec<(String, String, Option<Guarded>)>,
}

impl SlotFlow {
    /// The census entries the slots that the functions or bindings
    /// `roots` reach need.
    pub(super) fn guarded_from<'a>(&self, roots: impl Iterator<Item = &'a str>) -> Vec<Guarded> {
        let mut reached: HashSet<&str> = roots.collect();
        loop {
            let before = reached.len();
            for (v, s, _) in &self.hands {
                if reached.contains(v.as_str()) {
                    reached.insert(s.as_str());
                }
            }
            if reached.len() == before {
                break;
            }
        }
        let needs: HashSet<&Guarded> = (self.hands.iter())
            .filter(|(v, ..)| reached.contains(v.as_str()))
            .filter_map(|(_, _, g)| g.as_ref())
            .collect();
        needs.into_iter().cloned().collect()
    }
}

/// Does the census of the checked `mono` ast box `g`?
pub(super) fn boxed(ast: &Ast, mono: &Ast, g: &Guarded) -> bool {
    match g {
        Guarded::Param(f, p) => mono.boxed_params_of(f).is_some_and(|ps| ps.contains(p)),
        Guarded::Let(n) => {
            let mut hit = false;
            crate::ast::walk_stmts(&ast.stmts, &mut |s| {
                if let Stmt::LetDecl { name, init, .. } = s
                    && name == n
                {
                    hit |= mono.face_mismatch_lets.contains(init);
                }
            });
            hit
        }
    }
}

/// The names read as the value of a fn-typed slot of the face census:
/// a binding's initializer under a function-type annotation, or the
/// argument of a declared function's fn-typed parameter. A slot's value
/// read by its name may also be stored into a binding under a
/// function-type annotation, or handed to a fn-typed parameter of a
/// closure whose calls are all in sight; such a slot holds it only when
/// the census boxes it, which `allow` answers.
///
/// A slot whose name is read other than as a callee or as the value of
/// another such slot hands the function on to a slot typed with its
/// declared return (`xs.push(k)`, then `xs[0]()`), so its value is not
/// counted, nor the values of the slots it is read from. Names resolve
/// by spelling: a same-named binding elsewhere read as a value costs
/// the retype, never an answer.
pub(super) fn fn_slot_values(ast: &Ast, allow: &dyn Fn(&Guarded) -> bool) -> SlotFlow {
    let fn_ann = |a: &Option<String>| a.as_deref().is_some_and(crate::ast::is_fn_like_ann);
    let slot = |p: &Param| fn_ann(&p.type_ann).then(|| p.name.clone());
    let mut params: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let mut fn_lets: HashSet<String> = HashSet::new();
    // slot value → the slot it is stored in
    let mut slots: HashMap<ExprId, (String, Option<Guarded>)> = HashMap::new();
    let ident = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(_));
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::FnDecl {
            name, params: ps, ..
        } => {
            let user = ps.iter().filter(|p| !p.name.starts_with("__"));
            params.insert(name.clone(), user.map(slot).collect());
        }
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } if fn_ann(type_ann) => {
            fn_lets.insert(name.clone());
            if ident(*init) {
                slots.insert(*init, (name.clone(), None));
            }
        }
        _ => {}
    });
    let slot_names: HashSet<&str> = (fn_lets.iter().map(String::as_str))
        .chain(params.values().flatten().flatten().map(String::as_str))
        .collect();
    let slot_read = |e: ExprId| match ast.get_expr(e) {
        Expr::Ident(n) => slot_names.contains(n.as_str()) && !params.contains_key(n),
        _ => false,
    };
    // the callees, and the names a store writes: neither hands the
    // value on
    let mut callees: HashSet<ExprId> = HashSet::new();
    let mut closures: HashSet<&str> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Call { callee, args } => {
                callees.insert(*callee);
                let Expr::Ident(g) = ast.get_expr(*callee) else {
                    continue;
                };
                for (a, p) in args.iter().zip(params.get(g).into_iter().flatten()) {
                    if let Some(p) = p.as_ref().filter(|_| ident(*a)) {
                        slots.insert(*a, (p.clone(), None));
                    }
                }
            }
            Expr::OptCall { callee, .. } => {
                callees.insert(*callee);
            }
            Expr::Assign { target, value } => {
                callees.insert(*target);
                if let Expr::Ident(n) = ast.get_expr(*target)
                    && fn_lets.contains(n)
                    && slot_read(*value)
                {
                    let g = Guarded::Let(n.clone());
                    if allow(&g) {
                        slots.insert(*value, (n.clone(), Some(g)));
                    }
                }
            }
            Expr::Closure { fn_name, .. } => {
                closures.insert(fn_name);
            }
            _ => {}
        }
    }
    for f in closures {
        for (i, p) in params.get(f).into_iter().flatten().enumerate() {
            let Some(p) = p else { continue };
            let g = Guarded::Param(f.to_string(), p.clone());
            if !allow(&g) {
                continue;
            }
            for a in closure_args(ast, f, i).into_iter().flatten() {
                if slot_read(a) {
                    slots.insert(a, (p.clone(), Some(g.clone())));
                }
            }
        }
    }
    loop {
        let mut escaped: HashSet<&str> = HashSet::new();
        for (i, e) in ast.exprs.iter().enumerate() {
            let eid = ExprId(i as u32);
            if let Expr::Ident(n) = e
                && !callees.contains(&eid)
                && !slots.contains_key(&eid)
            {
                escaped.insert(n.as_str());
            }
        }
        let before = slots.len();
        slots.retain(|_, (s, _)| !escaped.contains(s.as_str()));
        if slots.len() == before {
            break;
        }
    }
    let hands = (slots.iter())
        .filter_map(|(v, (s, g))| match ast.get_expr(*v) {
            Expr::Ident(n) => Some((n.clone(), s.clone(), g.clone())),
            _ => None,
        })
        .collect();
    SlotFlow {
        values: slots.into_keys().collect(),
        hands,
    }
}

#[cfg(test)]
mod tests {
    use super::fn_slot_values;
    use crate::{lexer, parser};

    /// (name read, slot) for each read `src` hands to a slot.
    fn hands(src: &str) -> Vec<(String, String)> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let flow = fn_slot_values(&ast, &|_| true);
        let mut out: Vec<(String, String)> =
            (flow.hands.into_iter()).map(|(v, s, _)| (v, s)).collect();
        out.sort();
        out
    }

    fn pair(v: &str, s: &str) -> (String, String) {
        (v.to_string(), s.to_string())
    }

    #[test]
    fn a_slot_only_called_holds_the_function() {
        let src = "function h(): number { return 1 }
            const k: () => number = h;
            function run(f: () => number): number { return f() }
            console.log(k(), run(h), run(k));";
        assert_eq!(
            hands(src),
            vec![pair("h", "f"), pair("h", "k"), pair("k", "f")]
        );
    }

    #[test]
    fn a_binding_read_as_a_value_hands_the_function_on() {
        let src = "function h(): number { return 1 }
            const k: () => number = h;
            const xs: (() => number)[] = [];
            xs.push(k);
            console.log(xs[0]());";
        assert_eq!(hands(src), vec![]);
    }

    #[test]
    fn a_parameter_read_as_a_value_hands_the_function_on() {
        let src = "function h(): number { return 1 }
            const ys: (() => number)[] = [];
            function keep(f: () => number) { ys.push(f) }
            keep(h);
            const k: () => number = h;
            console.log(k());";
        assert_eq!(hands(src), vec![pair("h", "k")]);
    }

    #[test]
    fn a_store_of_the_function_itself_is_no_slot() {
        let src = "function h(): number { return 1 }
            let s: () => number = () => 2;
            s = h;
            const k: () => number = h;
            let t: () => number = () => 3;
            t = k;
            console.log(s(), k(), t());";
        assert_eq!(hands(src), vec![pair("h", "k"), pair("k", "t")]);
    }
}
