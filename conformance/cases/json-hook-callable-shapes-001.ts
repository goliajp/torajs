// §25.5.1 step 7 (reviver) and §25.5.2 step 4.a (replacer) both ask
// IsCallable — §7.2.3. tr asked for a closure-cell (env, boxed
// entry) pair instead, and the two other callable shapes have no
// pair, so a Proxy or a class in either slot read as "not callable".
//
// That is worse here than a refusal, and it is why the conformance
// gate could not see it: "not callable" in these two positions does
// not throw, it means "no hook was given". A written reviver was
// silently skipped and the document came back unfiltered; a written
// replacer was silently dropped and every property was emitted. Both
// answers are well-formed JSON, so nothing downstream could tell.
//
// Both hooks are now called through the ordinary explicit-`this`
// kernel, which holds all three shapes — and which is also what the
// spec means, since the `this` here is the holder and changes at
// every step of the walk.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const dbl: any = new Proxy(((_k: string, v: any) =>
  typeof v === "number" ? v * 2 : v) as any, {});
const trapped: any = new Proxy((() => 0) as any, {
  apply: (_t: any, _th: any, a: any) => (typeof a[1] === "number" ? a[1] * 5 : a[1]),
});
const doc = '{"a":1,"b":{"c":2},"d":[3,4]}';

console.log("revive proxy   ", attempt(() => JSON.stringify(JSON.parse(doc, dbl))));
console.log("revive trap    ", attempt(() => JSON.stringify(JSON.parse(doc, trapped))));
console.log("replace proxy  ", attempt(() => JSON.stringify({ a: 1, b: { c: 2 } }, dbl)));
console.log("replace trap   ", attempt(() => JSON.stringify({ a: 1, b: { c: 2 } }, trapped)));
// A class is callable, so it is CALLED — and §10.2.1 step 2 throws.
// Silently ignoring it was the bug; the throw is the spec's answer.
class K {}
console.log("revive class   ", attempt(() => JSON.parse(doc, K as any)));
console.log("replace class  ", attempt(() => JSON.stringify({ a: 1 }, K as any)));
// Genuinely not callable — still "no hook", which is correct here.
console.log("revive number  ", attempt(() => JSON.stringify(JSON.parse(doc, 5 as any))));
console.log("revive null    ", attempt(() => JSON.stringify(JSON.parse(doc, null as any))));
console.log("revive absent  ", attempt(() => JSON.stringify(JSON.parse(doc))));
console.log("replace number ", attempt(() => JSON.stringify({ a: 1 }, 5 as any)));
// §25.5.2 step 4.b — an Array in the slot is a property list, and
// the callable branch is taken first, so this is unaffected.
console.log("replace list   ", attempt(() => JSON.stringify({ a: 1, b: 2 }, ["a"] as any)));
console.log("replace absent ", attempt(() => JSON.stringify({ a: 1, b: 2 })));
// Plain closures, unmoved.
console.log("revive plain   ", attempt(() =>
  JSON.stringify(JSON.parse(doc, (_k: string, v: any) => (typeof v === "number" ? v * 2 : v)))));
console.log("replace plain  ", attempt(() =>
  JSON.stringify({ a: 1, b: { c: 2 } }, (_k: string, v: any) =>
    (typeof v === "number" ? v * 2 : v))));
console.log("space unmoved  ", attempt(() =>
  JSON.stringify({ a: 1 }, null as any, 2).replace(/\n/g, "|")));
