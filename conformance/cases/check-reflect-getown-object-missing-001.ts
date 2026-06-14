// W-H — Object.getOwnPropertyDescriptor(real-object, missing-key) returns undefined
// reflect.rs:200-204 fall-through path: if obj is a cell with no `__torajs_dynobj_has`
// hit for the key, the descriptor lookup returns undefined (bun parity).
// 5 shapes: object literal, array indexed out-of-range, multi-key object,
// empty array, typed class instance.

console.log(Object.getOwnPropertyDescriptor({ a: 1 }, "b"));
console.log(Object.getOwnPropertyDescriptor([1, 2, 3], "5"));
console.log(Object.getOwnPropertyDescriptor({ x: 1, y: 2 }, "z"));
console.log(Object.getOwnPropertyDescriptor([], "0"));
class P { p = 1; }
console.log(Object.getOwnPropertyDescriptor(new P(), "missing"));
