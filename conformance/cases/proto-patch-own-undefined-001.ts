// An own entry storing undefined is not an absent key. Assigning
// `undefined` to a builtin prototype method shadows the builtin
// surface with a real property that happens not to be callable
// (§10.1.8.1 OrdinaryGet, then §13.3.6.1 step 5), where an absent key
// leaves the surface showing through. The prototype own-probe answers
// ANY_UNDEF for both, so telling them apart takes a membership probe.

// --- the shadowing half ---
(Map.prototype as any).get = undefined;
const m: any = new Map([[1, "native"]]);
try {
  console.log("map: no throw", m.get(1));
} catch (e: any) {
  console.log("map:", e instanceof TypeError);
}

(String.prototype as any).toUpperCase = undefined;
const s: any = "abc";
try {
  console.log("str: no throw", s.toUpperCase());
} catch (e: any) {
  console.log("str:", e instanceof TypeError);
}

(Number.prototype as any).toFixed = undefined;
const n: any = 1.5;
try {
  console.log("num: no throw", n.toFixed(1));
} catch (e: any) {
  console.log("num:", e instanceof TypeError);
}

// --- the showing-through half: every OTHER name on those same
//     prototypes is untouched, so the builtin surface still answers ---
console.log("map others", m.has(1), m.size);
console.log("str others", s.toLowerCase(), s.length);
console.log("num others", n.toPrecision(2));

// Deleting the shadow is NOT tested here: both bun and node then
// answer `m.get is not a function`, because a builtin prototype
// method is an own property of that prototype and deleting it leaves
// nothing to inherit. torajs re-exposes its virtual surface instead —
// a separate axis (the builtin-proto delete tombstone is wired for
// `constructor` only), recorded rather than asserted.
