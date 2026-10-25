//! What a call can do to the ToPrimitive hooks
//! ([`crate::cmd_build_dispatch_judge_coerce_hooks`]), by callee. A
//! program function is scanned on its own, so a call of one does
//! nothing at the call. Every runtime entry is listed in exactly one
//! table here or in [`crate::cmd_build_dispatch_judge_coerce_inert`]
//! (a test lowers a program and checks each declaration), and an
//! entry no table lists answers [`Role::Installer`].

use torajs_core::ssa::Function;

use crate::cmd_build_dispatch_judge::MID_CARRIERS;
use crate::cmd_build_dispatch_judge_coerce_inert::{INERT_HOST, INERT_RUNTIME, listed};
use crate::cmd_build_dispatch_judge_ctor_uses::KEYED_READERS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Stores under no key and hands out no definer.
    Inert,
    /// Initializes an object the call site's code just minted (an
    /// object literal, a class body and its prologue links): the
    /// object is on no builtin chain until a relink puts it there.
    Fresh,
    /// Reads the property its operand 1 names.
    Reader,
    /// Stores under the key at `key`; `value` is the operand of the
    /// stored value when it is one `any`.
    Keyed { key: usize, value: Option<usize> },
    /// Calls or hands out the method its operand `mid` names by id.
    MethodCall { mid: usize },
    /// Can store under any key or relink a prototype.
    Installer,
}

/// The fresh-object initializers.
pub(crate) const FRESH: [&str; 21] = [
    "__torajs_anyv_proto_register",
    "__torajs_class_accessor_define",
    "__torajs_class_computed_accessor_define",
    "__torajs_class_computed_method_define",
    "__torajs_class_static_accessor_define",
    "__torajs_class_static_field_define",
    "__torajs_class_static_method_define",
    "__torajs_class_static_own_move_to_end",
    "__torajs_class_unit_static_accessor_define",
    "__torajs_class_unit_static_field_define",
    "__torajs_class_unit_static_method_define",
    "__torajs_class_unit_wire",
    // a generator factory's `prototype` and its per-generator proto
    "__torajs_closure_install_gen_proto",
    "__torajs_dynobj_mark_null_proto",
    "__torajs_dynobj_set_fresh",
    "__torajs_dynobj_set_fresh_dup",
    // `{ ...source }`: own data properties into the fresh literal
    "__torajs_dynobj_spread_from",
    "__torajs_genfn_chain",
    // `Object.create(proto)`: the child is the fresh end
    "__torajs_object_create_link_proto",
    "__torajs_proto_chain_builtin",
    "__torajs_proto_link_fresh",
];

/// Readers beyond [`KEYED_READERS`] whose operand 1 is the key.
pub(crate) const READERS_MORE: [&str; 2] =
    ["__torajs_any_member_get_pair", "__torajs_in_op_any_str"];

/// Keyed writes: (entry, key operand, value operand when one `any`).
pub(crate) const KEYED_WRITERS: [(&str, usize, Option<usize>); 25] = [
    ("__torajs_any_index_method_call", 1, None),
    ("__torajs_any_index_set_keyed", 1, None),
    ("__torajs_any_index_set_keyed_soft", 1, None),
    ("__torajs_any_member_set", 1, None),
    ("__torajs_any_member_set_priv", 1, None),
    ("__torajs_any_member_set_soft", 1, None),
    ("__torajs_any_member_set_with_receiver", 1, None),
    // `super[k] = v` stores onto `this`, `super[k]++` a number
    ("__torajs_anyv_super_prop_set", 1, Some(2)),
    ("__torajs_anyv_super_prop_update", 1, None),
    ("__torajs_arr_define", 1, None),
    ("__torajs_arrprops_set", 1, None),
    // `defineProperty(o, key, desc)`: an accessor under a fixed key
    // other than a hook's is never Got by ToPrimitive
    ("__torajs_dynobj_define", 1, None),
    ("__torajs_dynobj_define_binding", 1, None),
    ("__torajs_dynobj_define_from_desc", 1, None),
    ("__torajs_dynobj_define_from_desc_soft", 1, None),
    ("__torajs_dynobj_dict_set", 1, Some(2)),
    ("__torajs_dynobj_dict_set_entry", 1, Some(2)),
    ("__torajs_dynobj_set", 1, None),
    ("__torajs_fnprops_set", 1, None),
    // the global object's bindings, keyed by the binding's name
    ("__torajs_global_fn_declare", 0, Some(1)),
    ("__torajs_global_ident_set", 0, Some(1)),
    ("__torajs_global_var_bind", 0, None),
    ("__torajs_global_var_declare", 0, None),
    ("__torajs_global_var_sync", 0, None),
    ("__torajs_obj_field_set_exotic", 1, None),
];

/// Bulk definers, prototype relinks, the descriptor bag of every own
/// property (it holds the `__proto__` setter), and `eval`.
pub(crate) const INSTALLERS: [&str; 7] = [
    "__torajs_anyv_assign",
    "__torajs_anyv_get_property_descriptors",
    "__torajs_anyv_proto_member_set",
    "__torajs_anyv_set_prototype_of",
    "__torajs_dynobj_define_properties_from",
    "__torajs_global_eval_value",
    "__torajs_reflect_set_prototype_of",
];

const RUNTIME_PREFIX: &str = "__torajs_";

/// The role of a call of `callee`.
pub(crate) fn role(callee: &Function) -> Role {
    if !callee.blocks.is_empty() {
        return Role::Inert;
    }
    listed_role(&callee.name).unwrap_or(Role::Installer)
}

/// The role the tables give `name`, None when no table lists it.
pub(crate) fn listed_role(name: &str) -> Option<Role> {
    if let Some(&(_, mid)) = MID_CARRIERS.iter().find(|(n, _)| *n == name) {
        return Some(Role::MethodCall { mid });
    }
    if let Some(&(_, key, value)) = KEYED_WRITERS.iter().find(|(n, ..)| *n == name) {
        return Some(Role::Keyed { key, value });
    }
    if KEYED_READERS.contains(&name) || READERS_MORE.contains(&name) {
        return Some(Role::Reader);
    }
    if FRESH.contains(&name) {
        return Some(Role::Fresh);
    }
    if INSTALLERS.contains(&name) {
        return Some(Role::Installer);
    }
    let inert = match name.strip_prefix(RUNTIME_PREFIX) {
        Some(x) => listed(INERT_RUNTIME, x),
        None => INERT_HOST.contains(&name),
    };
    inert.then_some(Role::Inert)
}
