// Rotation 208 — §25.5.2.4 step 8.b: a property whose
// SerializeJSONProperty answers nothing has its whole
// `<sep>"key":<value>` segment omitted. The static shapes that
// always do (a callable slot, a Symbol slot, a setter-only or
// Void-returning accessor) were already skipped before any value
// existed; an `any` slot only settles it at runtime, and the
// any-lane walk reports all three verdicts as the undefined-Str
// sentinel — which the concat chain used to append as the literal
// text `undefined` under an emitted key.
//
// The value is now serialized before the separator and key are
// appended, which is also the spec's own order (step 8 runs
// SerializeJSONProperty, then decides whether to emit).

const u: any = undefined;
console.log("A", JSON.stringify({ a: 1, u: u }));
console.log("B", JSON.stringify({ u: u }));
console.log("C", JSON.stringify({ u: u, v: u }));
console.log("D", JSON.stringify({ u: u, z: 9 }));

function g(): undefined {
  return undefined;
}
console.log("E", JSON.stringify({ a: 1, u: g() }));

// The other two nothing-verdicts ride the same sentinel.
const f: any = function () {};
console.log("F", JSON.stringify({ a: 1, f: f }));
const sy: any = Symbol("z");
console.log("G", JSON.stringify({ a: 1, s: sy }));
console.log("H", JSON.stringify({ f: f, s: sy }));

// Values that DO serialize keep their key — null included, since
// step 8.b only omits *nothing*, never JS null.
const n: any = null;
console.log("I", JSON.stringify({ a: 1, n: n }));
const v: any = 5;
console.log("J", JSON.stringify({ a: 1, v: v }));
const st: any = "hi";
console.log("K", JSON.stringify({ st: st, a: 1 }));
const ob: any = { p: 1 };
console.log("L", JSON.stringify({ a: 1, o: ob, u: u }));
