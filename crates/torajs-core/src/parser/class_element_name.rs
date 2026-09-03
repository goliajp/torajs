//! §15.7.1 — the names a class element may not carry, and where.
//!
//! Four rules read the same PropName and differ only in the position
//! it sits in:
//!
//! | position                        | forbidden PropName      |
//! |---------------------------------|-------------------------|
//! | FieldDefinition                 | `constructor`           |
//! | static FieldDefinition          | `constructor`,`prototype` |
//! | static MethodDefinition         | `prototype`             |
//! | MethodDefinition, SpecialMethod | `constructor`           |
//!
//! A SpecialMethod is a getter, a setter, a generator, an async
//! method or an async generator — everything except a plain one,
//! which when it is named `constructor` IS the constructor.
//!
//! **`static constructor(){}` and `static get constructor(){}` are
//! not on the list.** `PrototypePropertyNameList` collects only the
//! non-static elements, so a static element spelled `constructor` is
//! an ordinary static member; both run under bun and every other
//! engine. tr refused them until rotation 575, which is why this
//! module's arrival is a subtraction as much as an addition.
//!
//! A COMPUTED name is never on any of the lists: PropName of a
//! ComputedPropertyName is empty, so `static ["prototype"](){}` and
//! `["constructor"] = 1` are legal however the key evaluates. Callers
//! pass the literal-name flag they already carry for that reason —
//! the parser's `consumed_computed_name` is exactly the spec's
//! LiteralPropertyName test.

/// Where a ClassElementName sits, which is all the rules differ by.
#[derive(Clone, Copy)]
pub(super) enum ClassElementPos {
    Field,
    /// `special` = getter / setter / generator / async — §15.7.1's
    /// SpecialMethod, the shapes a `constructor` may not take.
    Method {
        special: bool,
    },
}

/// `Some(message)` when §15.7.1 forbids this name in this position.
/// `is_literal_name` is false for a computed key, which is exempt.
pub(super) fn class_element_name_error(
    class: &str,
    name: &str,
    is_static: bool,
    is_literal_name: bool,
    pos: ClassElementPos,
) -> Option<String> {
    if !is_literal_name {
        return None;
    }
    let what = match (pos, is_static, name) {
        (ClassElementPos::Field, _, "constructor") => "class field",
        (ClassElementPos::Field, true, "prototype") => "static class field",
        (ClassElementPos::Method { .. }, true, "prototype") => "static class method",
        (ClassElementPos::Method { special: true }, false, "constructor") => {
            // A getter/setter/generator/async named `constructor` —
            // the plain method of that name is the constructor and
            // never reaches here with `special`.
            return Some(format!(
                "a getter, setter, generator or async method in class `{class}` \
                 may not be named `constructor` (ES §15.7.1)"
            ));
        }
        _ => return None,
    };
    Some(format!(
        "{what} in class `{class}` may not be named `{name}` (ES §15.7.1)"
    ))
}

use super::*;

impl Parser<'_> {
    /// `Err` when §15.7.1 forbids this ClassElementName in this
    /// position — the one-line form the three member parsers call.
    pub(super) fn reject_class_element_name(
        &self,
        class: &str,
        name: &str,
        is_static: bool,
        is_literal_name: bool,
        pos: ClassElementPos,
    ) -> Result<(), String> {
        match class_element_name_error(class, name, is_static, is_literal_name, pos) {
            Some(msg) => Err(format!("{msg} at {}", self.at())),
            None => Ok(()),
        }
    }
}
