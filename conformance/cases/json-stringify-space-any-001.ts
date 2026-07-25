// Rotation 208 — ES §25.5.2.1 steps 5-8, the `space` argument. It
// was lowered and dropped, so every indenting call answered the
// compact text. The runtime walk now carries a gap: each element /
// property sits on its own line indented one gap per nesting level,
// the closing bracket returns to the parent's level, and a property
// separator gains the space after its colon. An empty composite
// stays on one line, which is what makes `[]` and `{}` survive.
//
// A Number space is min(10, ToIntegerOrInfinity(space)) spaces (so 0,
// negatives and NaN mean no indent, and 100 caps at 10); a String
// space keeps its first 10 code units; wrappers unwrap first per
// step 5; anything else leaves the gap empty and the output is
// byte-identical to the compact form.
//
// This is the any lane. A statically shaped receiver keeps its
// compile-time unfold — routing it here would mean boxing away the
// frontend types that tell an `undefined` field from a null one —
// and splices the same indentation itself; see
// json-stringify-space-static-001.

const o: any = { a: 1, b: { c: [1, 2], d: "x" }, e: [] };
console.log(JSON.stringify(o, null, 2));
console.log("--");
console.log(JSON.stringify(o, null, "\t"));
console.log("--");

// Number normalization: floor, clamp at both ends, NaN is zero.
console.log(JSON.stringify(o, null, 0));
console.log(JSON.stringify(o, null, -3));
console.log(JSON.stringify(o, null, 3.7));
console.log(JSON.stringify(o, null, NaN));
console.log("A", JSON.stringify(o, null, 100) === JSON.stringify(o, null, 10));
console.log("--");

// Empty composites take no newline at all.
const e1: any = [];
const e2: any = {};
const e3: any = [[]];
const e4: any = { a: {} };
console.log(JSON.stringify(e1, null, 2), JSON.stringify(e2, null, 2));
console.log(JSON.stringify(e3, null, 2));
console.log(JSON.stringify(e4, null, 2));
console.log("--");

// A string gap keeps its first 10 code units.
console.log(JSON.stringify(o, null, "abcdefghijklmnop"));
console.log("--");

// Step 5 unwraps Number / String objects before the split.
console.log(JSON.stringify(o, null, new Number(2)));
console.log(JSON.stringify(o, null, new String("--")));
console.log("--");

// A scalar has nothing to indent.
const n5: any = 5;
const s5: any = "s";
const nu: any = null;
console.log(JSON.stringify(n5, null, 2), JSON.stringify(s5, null, 2), JSON.stringify(nu, null, 2));

// The step 8.a / 8.b verdicts keep working under a gap.
const au: any = [1, undefined, 2];
const ou: any = { u: undefined, k: 1 };
console.log(JSON.stringify(au, null, 2));
console.log(JSON.stringify(ou, null, 2));

// Depth accumulates one gap per level.
const deep: any = { a: { b: { c: { d: 1 } } } };
console.log(JSON.stringify(deep, null, 1));
