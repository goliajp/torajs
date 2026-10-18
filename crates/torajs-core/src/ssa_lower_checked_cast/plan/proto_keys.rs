//! The names an instance of a declared class answers from its class's
//! prototype chain: its methods and accessors, own and inherited. An
//! object copied into such an instance that holds a property under one
//! of them would lose it to the class's member, so the object cast
//! refuses it (`torajs-anyvalue::anyv_cast::own`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Ast, GEN_METHOD_PREFIX, GEN_RECV_PARAM, PropKey, Stmt, unmangle_key};

/// The keys of each class asked for so far, by class name.
pub(crate) type ProtoKeyCache = RefCell<HashMap<String, Rc<[PropKey]>>>;

/// The prototype-chain keys of `class`, each once, from `cache` when
/// asked before.
pub(super) fn proto_keys(ast: &Ast, cache: &ProtoKeyCache, class: &str) -> Rc<[PropKey]> {
    if let Some(keys) = cache.borrow().get(class) {
        return keys.clone();
    }
    let keys: Rc<[PropKey]> = collect(ast, class).into();
    cache.borrow_mut().insert(class.to_string(), keys.clone());
    keys
}

/// [`proto_keys`] computed. Private members (`#m`, spelled
/// `__priv_<C>__m`) are no property keys and stay out.
fn collect(ast: &Ast, class: &str) -> Vec<PropKey> {
    let mut chain: Vec<&str> = Vec::new();
    let mut cur = Some(class);
    while let Some(c) = cur
        && let Some((name, parent)) = ast.class_parents.get_key_value(c)
        && !chain.contains(&c)
    {
        chain.push(name.as_str());
        cur = parent.as_deref();
    }
    let privates: Vec<String> = chain.iter().map(|c| format!("__priv_{c}__")).collect();
    let mut keys: Vec<PropKey> = Vec::new();
    let mut push = |k: PropKey| {
        let private = k
            .as_str()
            .is_some_and(|s| privates.iter().any(|p| s.starts_with(p)));
        if !private && !keys.contains(&k) {
            keys.push(k);
        }
    };
    let mut accessors: Vec<&(String, PropKey)> = (ast.accessor_getters.keys())
        .chain(ast.accessor_setters.keys())
        .filter(|(owner, _)| chain.contains(&owner.as_str()))
        .collect();
    accessors.sort();
    for (_, prop) in accessors {
        push(prop.clone());
    }
    let accessor_fns: Vec<&str> = (ast.accessor_getters.iter())
        .chain(ast.accessor_setters.iter())
        .filter(|((owner, _), _)| chain.contains(&owner.as_str()))
        .map(|(_, f)| f.as_str())
        .collect();
    let owners = Owners::new(ast, &chain);
    // (owner, member, whether its body is an accessor's) per method
    // body the chain declares
    let members: Vec<(&str, &str, bool)> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => {
                let generator = params.first().is_some_and(|p| p.name == GEN_RECV_PARAM);
                let (owner, m) = owners.method_of(name, generator)?;
                Some((owner, m, accessor_fns.contains(&name.as_str())))
            }
            _ => None,
        })
        .collect();
    for &(owner, m, accessor) in &members {
        // a mono instance (`m$$_number`, an accessor face's
        // `p_get$$anywv`) stands for the decl it was made from, which
        // is listed under its own name
        let mono_of_decl = m
            .split_once("$$")
            .is_some_and(|(base, _)| (members.iter()).any(|&(o, b, _)| o == owner && b == base));
        if !(accessor || mono_of_decl) {
            push(unmangle_key(m));
        }
    }
    keys
}

/// Which class of a chain a `__cm_<C>__<m>` body is named for.
struct Owners<'a> {
    chain: &'a [&'a str],
    /// The classes whose names extend a chain class's with `__`: a body
    /// one of them prefixes is theirs, the longest prefix winning.
    longer: Vec<&'a str>,
    /// Every class, for a generator body's owner.
    classes: Vec<&'a str>,
}

impl<'a> Owners<'a> {
    fn new(ast: &'a Ast, chain: &'a [&'a str]) -> Owners<'a> {
        let classes: Vec<&str> = ast.class_parents.keys().map(String::as_str).collect();
        let longer = (classes.iter().copied())
            .filter(|l| {
                (chain.iter()).any(|c| {
                    l.len() > c.len() + 2 && l.starts_with(c) && l[c.len()..].starts_with("__")
                })
            })
            .collect();
        Owners {
            chain,
            longer,
            classes,
        }
    }

    /// The chain class and member a method body `fname` is named for:
    /// `__cm_<C>__<m>`, or, for a generator method's hoisted body
    /// (`generator`, its receiver parameter first), `__cm_gen_<C>__<m>`;
    /// the constructor is no member.
    fn method_of(&self, fname: &'a str, generator: bool) -> Option<(&'a str, &'a str)> {
        let (owner, m) = match fname.strip_prefix(GEN_METHOD_PREFIX) {
            Some(rest) if generator => longest(&self.classes, rest)?,
            _ => {
                let rest = fname.strip_prefix("__cm_")?;
                let hit = longest(self.chain, rest)?;
                match longest(&self.longer, rest) {
                    Some((l, _)) if l.len() > hit.0.len() => return None,
                    _ => hit,
                }
            }
        };
        let member = !(m.is_empty() || m == "ctor" || m.starts_with("ctor$$"));
        (member && self.chain.contains(&owner)).then_some((owner, m))
    }
}

/// The longest of `classes` that prefixes `rest` followed by `__`, and
/// what follows.
fn longest<'r>(classes: &[&str], rest: &'r str) -> Option<(&'r str, &'r str)> {
    let mut best: Option<(&str, &str)> = None;
    for c in classes {
        if let Some(m) = rest.strip_prefix(c).and_then(|r| r.strip_prefix("__"))
            && best.is_none_or(|(b, _)| c.len() > b.len())
        {
            best = Some((&rest[..c.len()], m));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    fn keys(src: &str, class: &str) -> Vec<String> {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let mut ast = crate::parser::parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        let mut keys: Vec<String> = (super::collect(&ast, class).iter())
            .map(|k| k.lossy().into_owned())
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn inherited_methods_and_accessors_are_listed_and_the_constructor_is_not() {
        let src =
            "class A { constructor() {} m(): number { return 1 } get g(): number { return 2 } }
class B extends A { n(): number { return 3 } }
new B();";
        assert_eq!(keys(src, "B"), ["g", "m", "n"]);
        assert_eq!(keys(src, "A"), ["g", "m"]);
    }

    #[test]
    fn a_private_name_is_left_out_and_a_public_one_spelled_like_it_is_not() {
        let src = "class A { #p(): number { return 1 } __private(): number { return this.#p() } }
new A();";
        assert_eq!(keys(src, "A"), ["__private"]);
    }

    #[test]
    fn a_body_belongs_to_the_longest_class_name_that_prefixes_it() {
        let src = "class A { m(): number { return 1 } }
class A__B { n(): number { return 2 } }
new A(); new A__B();";
        assert_eq!(keys(src, "A"), ["m"]);
        assert_eq!(keys(src, "A__B"), ["n"]);
    }

    #[test]
    fn a_generator_body_belongs_to_its_class_not_to_a_gen_prefixed_one() {
        let src = "class X { *g(): Generator<number> { yield 1 } }
class gen_X { h(): number { return 2 } }
new X(); new gen_X();";
        assert_eq!(keys(src, "gen_X"), ["h"]);
        assert_eq!(keys(src, "X"), ["g"]);
    }
}
