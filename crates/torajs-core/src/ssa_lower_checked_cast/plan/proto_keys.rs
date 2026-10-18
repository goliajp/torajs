//! The names an instance of a declared class answers from its class's
//! prototype chain: its methods and accessors, own and inherited. An
//! object copied into such an instance that owns a property under one
//! of them would lose it to the class's member, so the object cast
//! refuses it (`torajs-anyvalue::anyv_cast::own`).

use std::collections::HashSet;

use crate::ast::{Ast, PropKey, Stmt, unmangle_key};

/// The prototype-chain keys of `class`, each once. Private members
/// (`#m`, spelled `__priv_<C>__m`) are no property keys and stay out.
pub(super) fn proto_keys(ast: &Ast, class: &str) -> Vec<PropKey> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = Some(class.to_string());
    while let Some(c) = cur {
        if chain.contains(&c) {
            break;
        }
        cur = ast.class_parents.get(&c).cloned().flatten();
        chain.push(c);
    }
    let accessor_fns: HashSet<&str> = (ast.accessor_getters.values())
        .chain(ast.accessor_setters.values())
        .map(String::as_str)
        .collect();
    let mut keys: Vec<PropKey> = Vec::new();
    let mut push = |k: PropKey| {
        let private = k.as_str().is_some_and(|s| s.starts_with("__priv"));
        if !private && !keys.contains(&k) {
            keys.push(k);
        }
    };
    let mut accessors: Vec<&(String, PropKey)> = (ast.accessor_getters.keys())
        .chain(ast.accessor_setters.keys())
        .filter(|(owner, _)| chain.contains(owner))
        .collect();
    accessors.sort();
    for (_, prop) in accessors {
        push(prop.clone());
    }
    let decls: Vec<&str> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let declared: HashSet<&str> = decls.iter().copied().collect();
    for fname in decls {
        let Some((owner, m)) = method_of(ast, fname) else {
            continue;
        };
        if !chain.iter().any(|c| c == owner) || accessor_fns.contains(fname) {
            continue;
        }
        // a mono instance (`m$$_number`, an accessor face's
        // `p_get$$anywv`) stands for the decl it was made from, which
        // is listed under its own name
        if let Some((base, _)) = m.split_once("$$")
            && declared.contains(format!("__cm_{owner}__{base}").as_str())
        {
            continue;
        }
        push(unmangle_key(m));
    }
    keys
}

/// The class and member a `__cm_<C>__<m>` method body is named for:
/// the longest class name that prefixes it (a class name may itself
/// contain `__`). The constructor is no member.
fn method_of<'n>(ast: &'n Ast, fname: &'n str) -> Option<(&'n str, &'n str)> {
    let rest = fname.strip_prefix("__cm_")?;
    let mut best: Option<(&str, &str)> = None;
    for cname in ast.class_parents.keys() {
        if let Some(m) = (rest.strip_prefix(cname.as_str())).and_then(|r| r.strip_prefix("__"))
            && best.is_none_or(|(b, _)| cname.len() > b.len())
        {
            best = Some((cname.as_str(), m));
        }
    }
    let (owner, m) = best?;
    (!(m.is_empty() || m == "ctor" || m.starts_with("ctor$$"))).then_some((owner, m))
}
