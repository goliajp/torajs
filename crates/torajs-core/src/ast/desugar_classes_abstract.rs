//! `desugar_classes` abstract-class collection + validation (chunk 178,
//! 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (Pass 1 sub-section just
//! after class_index construction). Pure read of `ast.stmts` +
//! `class_index`; returns the `abstract_classes` HashSet consumed
//! later in Pass 3 for `new`-rejection checks. The `abstract_methods`
//! HashMap is local — only consumed by the validation walk inside
//! this sub-fn, so it never crosses the call boundary.
//!
//! M-OO.6 rules enforced here:
//!   * abstract method only allowed inside abstract class
//!     (parser already rejects the immediate case; this catches
//!     programmatically-built classes from upstream desugars).
//!   * concrete class must override every inherited abstract method
//!     (walks each concrete class's inheritance chain root → leaf,
//!     accumulating unimplemented abstract names).
//!
//! Both validations panic on violation (preserving pre-extract
//! behavior). Body verbatim.

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn collect_abstract_classes(
    ast: &Ast,
    class_index: &[ClassIndexEntry],
) -> HashSet<String> {
    // M-OO.6 — collect abstract-class names + per-class abstract-method
    // names. Concrete subclasses must override every inherited abstract;
    // `new` of an abstract class is rejected (in check.rs). Side-channel
    // (HashSet / HashMap) instead of inflating class_index's tuple.
    let mut abstract_classes: HashSet<String> = HashSet::new();
    let mut abstract_methods: HashMap<String, Vec<String>> = HashMap::new();
    for s in ast.stmts.iter() {
        if let Stmt::ClassDecl {
            name,
            is_abstract,
            methods,
            ..
        } = s
        {
            if *is_abstract {
                abstract_classes.insert(name.clone());
            }
            let abs: Vec<String> = methods
                .iter()
                .filter(|m| m.is_abstract)
                .map(|m| m.name.clone())
                .collect();
            if !abs.is_empty() {
                abstract_methods.insert(name.clone(), abs);
            }
            // Abstract method only allowed inside abstract class.
            // (Parser already rejects this for the immediate case, but
            // a desugar-time double-check catches programmatically-built
            // classes from upstream desugars.)
            if !is_abstract && methods.iter().any(|m| m.is_abstract) {
                panic!("M-OO.6: concrete class `{name}` cannot declare abstract methods");
            }
        }
    }
    // Walk every concrete class's inheritance chain (root → leaf,
    // accumulating "unimplemented" abstract names along the way) and
    // verify that none survive into the concrete leaf.
    for (_, cname, _, _, _, _, _, _, _) in class_index {
        if abstract_classes.contains(cname) {
            continue;
        }
        let mut chain: Vec<String> = Vec::new();
        let mut cur: Option<String> = Some(cname.clone());
        while let Some(c) = cur {
            chain.push(c.clone());
            cur = class_index
                .iter()
                .find(|t| t.1 == c)
                .and_then(|t| t.3.clone());
            // A mutual `extends` cycle (a → b → a) must not spin the
            // walk — the checker's declared-before rule rejects the
            // program right after this pass, loudly. (Direct
            // self-extends never gets here: the parser rewrites it to
            // the TDZ throw, see parser/class_self_heritage.rs.)
            if let Some(next) = &cur
                && chain.contains(next)
            {
                break;
            }
        }
        chain.reverse();
        let mut unimplemented: HashSet<String> = HashSet::new();
        for cls in &chain {
            if let Some(absms) = abstract_methods.get(cls) {
                for m in absms {
                    unimplemented.insert(m.clone());
                }
            }
            if let Some(t) = class_index.iter().find(|t| &t.1 == cls) {
                let cls_methods = &t.7;
                for m in cls_methods.iter() {
                    if !m.is_abstract {
                        unimplemented.remove(&m.name);
                    }
                }
            }
        }
        if !unimplemented.is_empty() {
            let mut names: Vec<&String> = unimplemented.iter().collect();
            names.sort();
            panic!("M-OO.6: concrete class `{cname}` must override abstract method(s): {names:?}");
        }
    }
    abstract_classes
}
