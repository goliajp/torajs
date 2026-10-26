//! The type of a binding initialized with an empty array literal.

use crate::check::{Checker, DiagPush, Type};
use crate::check_type_ann::{resolve_type_ann_full, unknown_type};

pub(super) fn check_empty_array_ann(
    checker: &mut Checker,
    name: &str,
    type_ann: &Option<String>,
) -> Option<Type> {
    match type_ann {
        Some(ann) => {
            let Some(t) =
                resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls)
            else {
                checker.errors.push_err(unknown_type(ann));
                return None;
            };
            // An `any` annotation absorbs the literal (chunk-809
            // any-ann family; rotation 73 L3b): `const e: any = []`
            // is a plain Arr<Any> boxed into the any slot — bun
            // accepts. Only a non-array, non-any annotation is a
            // real mismatch.
            if matches!(t, Type::Any) {
                return Some(Type::Array(Box::new(Type::Any)));
            }
            if !matches!(t, Type::Array(_)) {
                checker.errors.push_err(format!(
                    "empty array literal `{name}` needs an array type annotation, got `{ann}`"
                ));
                return None;
            }
            Some(t)
        }
        None => Some(Type::Array(Box::new(Type::Any))),
    }
}
