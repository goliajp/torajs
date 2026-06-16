// S130-4 narrow: `<v: any> instanceof <BuiltinClass>` Any-operand
// support for Array / Date / RegExp / Promise / Map / Set / WeakMap /
// WeakSet. Pre-fix the static-fold catch-all arm `("Array", _) =>
// Some(false)` matched Type::Any operands first and returned a
// constant false, masking the runtime tag check. Mirror form of the
// existing user-class Any branch (which dispatches per descendant
// class_tag via `__torajs_instanceof_class_any_tag`); built-ins read
// the universal HeapHeader `type_tag@+4` via a sibling helper
// `__torajs_instanceof_builtin_any_tag`.
const arr: any = [1, 2, 3];
const d: any = new Date(0);
const re: any = /abc/;
const p: any = Promise.resolve(1);
const m: any = new Map<string, number>();
const s: any = new Set<string>();
const wm: any = new WeakMap<object, number>();
const ws: any = new WeakSet<object>();

// Each built-in class matches its own tag.
console.log(arr instanceof Array);
console.log(d instanceof Date);
console.log(re instanceof RegExp);
console.log(p instanceof Promise);
console.log(m instanceof Map);
console.log(s instanceof Set);
console.log(wm instanceof WeakMap);
console.log(ws instanceof WeakSet);

// Cross-tag mismatch — `arr instanceof Date` must be false because
// the runtime tag check reads Arr=2 and compares to Date=5. Confirms
// the new helper actually inspects type_tag (vs always-true).
console.log(arr instanceof Date);
console.log(d instanceof Array);
console.log(re instanceof Promise);
console.log(p instanceof RegExp);
console.log(m instanceof Set);
console.log(s instanceof Map);
console.log(wm instanceof WeakSet);
console.log(ws instanceof WeakMap);

// Primitives via Any → all false (subset has no boxed wrappers).
const n: any = 42;
const str: any = "hi";
console.log(n instanceof Number);
console.log(str instanceof String);
console.log(n instanceof Array);
