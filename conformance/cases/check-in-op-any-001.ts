// S131-1 narrow: `<key> in <obj: any>` runtime tag dispatch. Pre-fix
// the Type::Any rhs arm in ssa_lower's T-45 `__torajs_in_op` lowering
// unconditionally called `__torajs_dynobj_has` on the unboxed payload,
// SIGSEGV'ing when the Any cell was actually an Array (Array heap
// layout ≠ DynObj layout). Mirror form of the existing instanceof-Any
// runtime-tag pattern (`__torajs_instanceof_builtin_any_tag` / `_object_any`):
// two helpers (__torajs_in_op_any_num for Number-key, _str for String-
// key) that read the universal HeapHeader `type_tag@+4` and dispatch
// Tag::Arr (bounds check) / Tag::DynObj (dynobj_has) — every other
// cell tag collapses to false. See torajs-rc::in_op_any.

// Number-keyed in / Array-tagged Any rhs.
const arr: any = [10, 20, 30];
console.log(0 in arr);
console.log(2 in arr);
console.log(3 in arr);
console.log(-1 in arr);

// String-keyed in / DynObj-tagged Any rhs.
const obj: any = {};
obj.foo = 1;
obj.bar = "hi";
console.log("foo" in obj);
console.log("bar" in obj);
console.log("baz" in obj);

// Spec dynamic property lookup ToString(key) — `0 in obj` is true
// when `obj["0"]` exists. `__torajs_in_op_any_num` Tag::DynObj
// branch ToString-coerces the i64 key via `__torajs_num_to_string`
// and queries dynobj_has, mirroring the symmetric Array str-numeric-
// coerce path above. Use defineProperty to install numeric-string
// keys (`obj["0"] = …` would be rejected by check.rs's strict index
// rule when obj is Any-typed).
Object.defineProperty(obj, "0", {
    value: "zero",
    writable: true,
    enumerable: true,
    configurable: true,
});
Object.defineProperty(obj, "42", {
    value: "fortytwo",
    writable: true,
    enumerable: true,
    configurable: true,
});
console.log(0 in obj);
console.log(42 in obj);
console.log(1 in obj);
console.log(-1 in obj);

// Spec ECMA-262 §7.1.21 CanonicalNumericIndexString — `"0" in arr`
// is true because `"0"` is the canonical index-string of 0 and Array
// `[[HasProperty]]` accepts canonical index strings. Non-canonical
// shapes ("01", "+0", "-0", "1.5", "foo", "") all collapse to false.
console.log("0" in arr);
console.log("2" in arr);
console.log("3" in arr);
console.log("01" in arr);
console.log("+0" in arr);
console.log("-0" in arr);
console.log("1.5" in arr);
console.log("foo" in arr);
console.log("" in arr);
