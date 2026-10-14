//! What a slot's annotation says the plan's nodes are: whether it
//! admits nullish values, and the annotations and optional members of
//! the fields of the object type it names. Moved out of [`super`].

use std::collections::{HashMap, HashSet};

use crate::ast::Ast;
use crate::ssa::Type;

/// Whether the annotation spells `T | null | undefined` (an optional
/// member's among them).
pub(super) fn nullable(ann: Option<&str>) -> bool {
    ann.is_some_and(|a| a.trim().starts_with("__nullable("))
}

/// Whether the node of a slot of type `ty` annotated `ann` is an `OPT`
/// node. An optional `number` member keeps an f64 slot: the node admits
/// `undefined` (and a key the value lacks) as the slot's sentinel.
pub(super) fn opt_node(ty: Type, ann: Option<&str>) -> bool {
    nullable(ann) && matches!(ty, Type::Str | Type::Obj(_) | Type::Arr(_) | Type::F64)
}

/// Field annotations keyed by the field name's bytes, and the fields
/// declared optional.
pub(super) type FieldAnns = (HashMap<Vec<u8>, String>, HashSet<Vec<u8>>);

/// The field annotations of the object type `owner` names: an object
/// type literal's, or a declared type's (a class's among them; a generic
/// instance's, the fields every instance lays out alike), with the
/// members it declares optional.
pub(super) fn field_anns(ast: &Ast, owner: Option<&str>) -> Option<FieldAnns> {
    let owner = owner?;
    if let Some(fields) = crate::ast_collect_fn_closure_init::inlobj_fields(owner) {
        let optional = fields
            .iter()
            .filter(|f| f.1)
            .map(|f| f.0.clone().into_bytes());
        let optional = optional.collect();
        let anns = fields.into_iter().map(|(k, _, a)| (k.into_bytes(), a));
        return Some((anns.collect(), optional));
    }
    let (decl, fields) = crate::ast_throw_info::obj_cast::decl_fields(ast, owner)?;
    let members = &ast.optional_members;
    let optional = (fields.iter())
        .filter(|(k, _)| members.declared(decl, k) || members.plain_at(decl, k))
        .map(|(k, _)| k.as_wtf8().as_bytes().to_vec())
        .collect();
    let anns = (fields.into_iter()).map(|(k, a)| (k.as_wtf8().as_bytes().to_vec(), a));
    Some((anns.collect(), optional))
}
