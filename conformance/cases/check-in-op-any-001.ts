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

// Cross-tag negative — number-key vs DynObj rhs / string-key vs Array
// rhs both collapse to false in this narrow ship (DynObj numeric-key
// and Array numeric-string-coercion both deferred to L3b).
console.log(0 in obj);
console.log(1 in obj);
