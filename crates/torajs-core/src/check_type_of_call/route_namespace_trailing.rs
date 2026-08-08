//! Cascade segment 5/6 of [`crate::check_type_of_call::check`]'s
//! early-route chain. Wedge order preserved verbatim from the
//! pre-split cascade; segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings — relative order of
//! every arm is unchanged.
//!
//! Covers: array_reduce_trailing / array_at_slice_join / index_search_trailing /
//! object_keys_ownkeys / object_entries_freeze / object_reflect_3arg /
//! primitive_proto_trailing / struct_proto_trailing /
//! date_instance_trailing / number_tolocale_trailing /
//! symbol_static_trailing / mapset_add_set / object_static_proto /
//! date_setter / array_isarray_trailing / regexp_test_exec

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // S245/S276 — `Array<T>.{reduce,reduceRight}(fn, init,
    // ...trailing)` Array-receiver trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_array_reduce_trailing`]
    // (chunk 295).
    if let Some(r) =
        crate::check_type_of_call_array_reduce_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S242/S299 — `arr.{at,slice,join}(useful, ...trailing)`
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_at_slice_join`]
    // (chunk 294).
    if let Some(r) =
        crate::check_type_of_call_array_at_slice_join::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S239 — String/Array.{indexOf,lastIndexOf,includes}
    // + String.{startsWith,endsWith}(needle, fromIndex,
    // ...trailing) trailing-arg ignore per ES
    // §22.1.3.{8,10,5,21,7} / §23.1.3.{14,17,18}: spec
    // reserves slots after fromIndex but tora's helpers
    // are 2-arg only. Trim trailing operands at lower-time
    // (ssa_lower mirrors break early past i=1 / drop
    // args[2..]). Same shape as S238 localeCompare.
    //
    // S278 — `recv.{indexOf,lastIndexOf,includes,startsWith,
    // endsWith}(needle, fromIndex, ...trailing)` String- +
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_index_search_trailing`]
    // (chunk 265).
    if let Some(r) =
        crate::check_type_of_call_index_search_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Object.{keys,getOwnPropertyNames}(obj, ...trailing)` /
    // `Reflect.ownKeys(obj, ...trailing)` namespace
    // 1-useful-arg trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_keys_ownkeys`]
    // (chunk 293).
    if let Some(r) =
        crate::check_type_of_call_object_keys_ownkeys::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Object.{entries,freeze,isFrozen,values}(obj,
    // ...trailing)` Object-namespace 1-useful-arg trailing-
    // arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_entries_freeze`]
    // (chunk 292).
    if let Some(r) =
        crate::check_type_of_call_object_entries_freeze::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S270/S271/S272 — `Object.{hasOwn,is}(target, key,
    // ...trailing)` / `Reflect.{has,get}(target, key,
    // ...trailing)` namespace 3+arg trailing-arg ignore
    // wedge extracted to
    // [`crate::check_type_of_call_object_reflect_3arg`]
    // (chunk 291).
    if let Some(r) =
        crate::check_type_of_call_object_reflect_3arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // Boolean / Symbol / String primitive `toString` /
    // `toLocaleString` trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_primitive_proto_trailing`]
    // (chunk 290).
    if let Some(r) =
        crate::check_type_of_call_primitive_proto_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S304 — Struct-instance Object.prototype methods
    // trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_struct_proto_trailing`]
    // (chunk 289).
    if let Some(r) =
        crate::check_type_of_call_struct_proto_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S261 — Date instance 0-arg getter / format method
    // family trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_date_instance_trailing`]
    // (chunk 288).
    if let Some(r) =
        crate::check_type_of_call_date_instance_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S260 — `n.toLocaleString(locales?, options?, ...trailing)`
    // Number-receiver 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_number_tolocale_trailing`]
    // (chunk 287).
    if let Some(r) =
        crate::check_type_of_call_number_tolocale_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S259 — `Symbol.{for,keyFor}(key, ...trailing)`
    // namespace trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_symbol_static_trailing`]
    // (chunk 286).
    if let Some(r) =
        crate::check_type_of_call_symbol_static_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S248 — `Set.add(value, ...trailing)` / `Map.set(key,
    // value, ...trailing)` Set/Map-receiver trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_mapset_add_set`]
    // (chunk 266).
    if let Some(r) = crate::check_type_of_call_mapset_add_set::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S269 + S317 — `Object.{create,setPrototypeOf,
    // defineProperties,defineProperty}(...)` Object-namespace
    // trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_static_proto`]
    // (chunk 267).
    if let Some(r) =
        crate::check_type_of_call_object_static_proto::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S268 — `d.set{FullYear,Month,Date,Hours,Minutes,
    // Seconds,Milliseconds,Time,Year}(..., ...trailing)`
    // Date-instance setter trailing-arg ignore wedge
    // extracted to [`crate::check_type_of_call_date_setter`]
    // (chunk 268).
    if let Some(r) = crate::check_type_of_call_date_setter::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S267 — `Array.isArray(value, ...trailing)` Array-
    // namespace trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_isarray_trailing`]
    // (chunk 269).
    if let Some(r) =
        crate::check_type_of_call_array_isarray_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S266 — `r.{test,exec,toString}(s?, ...trailing)`
    // RegExp-receiver trailing-arg ignore wedge extracted
    // to [`crate::check_type_of_call_regexp_test_exec`]
    // (chunk 270).
    if let Some(r) =
        crate::check_type_of_call_regexp_test_exec::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // RFC 20260808-json-parse-any blade 3 — `JSON.parse(text,
    // reviver?, ...trailing)` arity wedge (0-2 useful slots).
    if let Some(r) = crate::check_type_of_call_json_parse::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    None
}
