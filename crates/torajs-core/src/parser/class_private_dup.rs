//! §15.7.1 ClassBody — PrivateBoundIdentifiers may not repeat.
//!
//! > It is a Syntax Error if PrivateBoundIdentifiers of
//! > ClassElementList contains any duplicate entries, unless the name
//! > is used once for a getter and once for a setter and in no other
//! > entries, and the getter and setter are either both static or
//! > both non-static.
//!
//! So `class C { #x(){} #x(){} }`, `{ #x = 1; #x(){} }` and
//! `{ static get #x(){} set #x(v){} }` are all Syntax Errors, while
//! `{ get #x(){} set #x(v){} }` and its all-static twin are the one
//! legal repeat. tr accepted every one of them until rotation 575 —
//! the plain method-method case reached ssa_lower and died there as
//! `redeclaration of function __cm_C____priv_C__x`, which is the
//! right answer arriving in the wrong phase and under a name that
//! names an implementation detail.
//!
//! **The rule is a property of the whole ClassElementList**, not of
//! any one member, so it is checked where the list is complete and
//! the private scope closes — `pop_class_scope` below replaces the
//! bare `class_stack.pop()` the class-body parser used to call. A
//! member-by-member check would have to answer "is this the setter
//! half of a pair" before the other half has been parsed.
//!
//! Names arrive already mangled to `__priv_<C>__<raw>`, which is what
//! keeps two classes' `#x` apart (and a nested class's from its
//! outer's) without this pass knowing anything about scopes.

use super::*;
use std::collections::HashMap;

/// What a private ClassElementName was declared as. `Other` covers
/// fields and plain / generator / async methods — everything that can
/// never be half of a legal pair.
#[derive(Clone, Copy, PartialEq)]
enum PrivKind {
    Other,
    Getter,
    Setter,
}

fn kind_of(m: &ClassMethod) -> PrivKind {
    match m.accessor_kind {
        Some(ast::AccessorKind::Getter) => PrivKind::Getter,
        Some(ast::AccessorKind::Setter) => PrivKind::Setter,
        None => PrivKind::Other,
    }
}

/// The `#x` a mangled member name came from, or `None` when the
/// member is not private to THIS class.
fn private_raw(class: &str, key: &PropKey) -> Option<String> {
    let prefix = format!("__priv_{class}__");
    key.as_str()?.strip_prefix(&prefix).map(str::to_string)
}

/// The one legal repeat: exactly two declarations, one getter and one
/// setter, agreeing on static-ness.
fn is_legal_pair(decls: &[(PrivKind, bool)]) -> bool {
    matches!(decls, [(a, sa), (b, sb)] if sa == sb
        && ((*a == PrivKind::Getter && *b == PrivKind::Setter)
            || (*a == PrivKind::Setter && *b == PrivKind::Getter)))
}

impl Parser<'_> {
    /// Close the innermost class's private scope, refusing a class
    /// body whose private names repeat (§15.7.1, see module doc).
    pub(super) fn pop_class_scope(
        &mut self,
        class: &str,
        fields: &[(PropKey, String)],
        static_init: &[StaticInit],
        methods: &[ClassMethod],
        static_methods: &[ClassMethod],
    ) -> Result<(), String> {
        let mut seen: HashMap<String, Vec<(PrivKind, bool)>> = HashMap::new();
        let mut note = |key: &PropKey, kind: PrivKind, is_static: bool| {
            if let Some(raw) = private_raw(class, key) {
                seen.entry(raw).or_default().push((kind, is_static));
            }
        };
        for (k, _) in fields {
            note(k, PrivKind::Other, false);
        }
        for si in static_init {
            if let StaticInit::Field(f) = si {
                note(&f.name, PrivKind::Other, true);
            }
        }
        for m in methods {
            note(&m.name, kind_of(m), false);
        }
        for m in static_methods {
            note(&m.name, kind_of(m), true);
        }
        for (raw, decls) in &seen {
            if decls.len() > 1 && !is_legal_pair(decls) {
                return Err(format!(
                    "class `{class}` declares `#{raw}` more than once; only a \
                     getter and a setter of matching static-ness may share a \
                     private name (ES §15.7.1) at {}",
                    self.at()
                ));
            }
        }
        self.class_stack.pop();
        Ok(())
    }
}
