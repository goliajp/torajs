// rotation 140 — a throwing callback aborts the typed-tier HO loops
// (ReturnIfAbrupt at the per-element Call). test262 Set/prototype/
// forEach/throws-when-callback-throws: the emitted iterator loops
// (Array HO family / Map forEach / Set forEach) never checked the
// pending throw after the callback call — the loop ran every
// remaining element and the throw vanished at the statement edge.
// emit_throw_check after the invoke (devirt'd callbacks ride the
// may-throw gate) ends the walk and propagates.

// Array forEach: aborts on the first element.
let c1 = 0;
try {
  [1, 2, 3].forEach(function () { c1++; throw new Error("a"); });
  console.log("arr-no-throw");
} catch (e: any) { console.log("arr:", e instanceof Error, e.message); }
console.log("c1:", c1);

// Array map: same abort, no partial result leaks.
let c2 = 0;
try {
  [10, 20, 30].map(function (x: number) { c2++; if (x === 20) { throw new RangeError("m"); } return x * 2; });
  console.log("map-no-throw");
} catch (e: any) { console.log("map:", e instanceof RangeError, e.message); }
console.log("c2:", c2);

// Array reduce: abort mid-fold.
let c3 = 0;
try {
  [1, 2, 3, 4].reduce(function (acc: number, x: number) { c3++; if (x === 3) { throw new Error("r"); } return acc + x; }, 0);
  console.log("reduce-no-throw");
} catch (e: any) { console.log("reduce:", e.message); }
console.log("c3:", c3);

// Map forEach.
const m = new Map([["k1", 1], ["k2", 2]]);
let c4 = 0;
try {
  m.forEach(function () { c4++; throw new Error("mm"); });
  console.log("mapfe-no-throw");
} catch (e: any) { console.log("mapfe:", e.message); }
console.log("c4:", c4);

// Set forEach (the test262 shape).
const s = new Set([1, 2, 3]);
let c5 = 0;
try {
  s.forEach(function () { c5++; throw new Error("ss"); });
  console.log("setfe-no-throw");
} catch (e: any) { console.log("setfe:", e.message); }
console.log("c5:", c5);

// Non-throwing controls keep their answers. (A typed `v: number`
// param on a Set-forEach callback reads raw box bits — recorded
// L3b face, not this blade; count-only callback here.)
console.log([1, 2, 3].map(function (x: number) { return x + 1; }).join(","));
let calls = 0;
const s2 = new Set([4, 5]);
s2.forEach(function () { calls++; });
console.log("calls:", calls);
