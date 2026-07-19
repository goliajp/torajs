// Object.prototype methods read as values off an Any receiver.
// builtin_method_supported declared these per receiver shape, and a
// plain object's list carried toLocaleString but not toString, so
// `const m = o.toString` came back undefined while `o.toString()`
// answered "[object Object]" -- callable but not readable.
// isPrototypeOf had the same split on every shape.
const o: any = { a: 1 };
// (`t.call(o)` is a separate unsupported shape -- calling .call on
// an Any value is a loud compile reject -- so the read is checked
// through typeof plus the direct call below.)
const t = o.toString;
const ip = o.isPrototypeOf;
console.log(typeof t, typeof ip);
console.log(o.toString(), o.isPrototypeOf(o));
const empty: any = {};
console.log(typeof empty.toString, empty.toString());
// Shadowing a prototype method is visible to typeof for the names
// whose value read is trusted. constructor / toString /
// toLocaleString still answer "function" here -- both gaps are
// tracked in plan-state L3b, so they are not asserted.
const s: any = {
  valueOf: undefined,
  hasOwnProperty: 42,
  propertyIsEnumerable: undefined,
  isPrototypeOf: "x",
};
console.log(typeof s.valueOf, typeof s.hasOwnProperty);
console.log(typeof s.propertyIsEnumerable, typeof s.isPrototypeOf);
// Unshadowed receivers keep answering function.
const arr: any = [1, 2];
console.log(typeof arr.valueOf, typeof arr.hasOwnProperty, typeof arr.isPrototypeOf);
const str: any = "ab";
console.log(typeof str.valueOf, typeof str.hasOwnProperty);
// A typed receiver keeps the by-name answer: a struct instance has
// no runtime member-read path (`.name` lowers to a field access), so
// the shortcut is what stops `typeof inst.hasOwnProperty` from
// becoming a lowering error. Same for a class instance.
class P {
  x: number = 1;
}
const inst = new P();
console.log(typeof inst.hasOwnProperty, typeof inst.valueOf);
console.log(typeof inst.toString, typeof inst.isPrototypeOf);
const lit = { x: 1, y: "s" };
console.log(typeof lit.hasOwnProperty, typeof lit.propertyIsEnumerable);
