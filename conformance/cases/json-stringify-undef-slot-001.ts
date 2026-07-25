// Rotation 208 — §25.5.2.4 step 8.b for the shape SSA cannot
// express on its own. A frontend `undefined` field lowers to the
// same pointer-shaped slot as one holding JS null, and the value
// recursion answered `"null"` for both — so `{ u: undefined }`
// serialized as `{"u":null}` where the spec omits the key entirely,
// while `{ n: null }` correctly keeps it.
//
// The checker does keep the two apart, so JSON.stringify now carries
// the argument's frontend type down its shape recursion (peeling
// Array element / Struct field types in step with the SSA walk) and
// settles the verdict before the slot is even loaded.

console.log("A", JSON.stringify({ u: undefined }));
console.log("B", JSON.stringify({ u: undefined, k: 1 }));
console.log("C", JSON.stringify({ k: 1, u: undefined }));
console.log("D", JSON.stringify({ a: 1, u: undefined, b: 2 }));

// null is NOT omitted — step 8.b only drops *nothing*.
console.log("E", JSON.stringify({ n: null }));
console.log("F", JSON.stringify({ n: null, u: undefined }));
console.log("G", JSON.stringify({ u: undefined, n: null }));

// One level down, the field types keep pace with the shape walk.
console.log("H", JSON.stringify({ a: { b: undefined, c: 2 } }));
console.log("I", JSON.stringify({ a: { b: undefined } }));
console.log("J", JSON.stringify({ o: { n: null, u: undefined }, k: 3 }));

// Mixed with the value kinds that share the field lane.
console.log("K", JSON.stringify({ s: "x", u: undefined, t: true }));
console.log("L", JSON.stringify({ arr: [1, 2], u: undefined }));
console.log("M", JSON.stringify({ u: undefined, arr: [undefined] }));

// Through a binding the whole object degrades to the any lane, which
// has always answered this correctly — it stays correct.
const o = { u: undefined, k: 1 };
console.log("N", JSON.stringify(o));
