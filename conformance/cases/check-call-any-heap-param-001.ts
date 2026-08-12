// r380 — TS any-assignability at the call boundary for HEAP-typed
// params. `const a: any = fn; takes(a)` against `takes(f: () => void)`
// used to be a loud `expected Function([], Void), got Any`; the callee
// is now cloned with that param widened to `any`, so the clone's body
// re-lowers through the any-tier lanes. Scalar params already rode a
// caller-side coerce; these four have no scalar to coerce into.

function takesFn(f: () => void) { f(); }
const a: any = function () { console.log("fn ok"); };
takesFn(a);

function takesArr(xs: number[]) { console.log(xs.length); }
const b: any = [1, 2, 3];
takesArr(b);

interface P { x: number }
function takesStruct(p: P) { console.log(p.x); }
const c: any = { x: 42 };
takesStruct(c);

function takesMap(m: Map<string, number>) { console.log(m.get("k")); }
const d: any = new Map<string, number>([["k", 9]]);
takesMap(d);

function takesSet(s: Set<number>) { console.log(s.size); }
const e: any = new Set<number>([1, 2]);
takesSet(e);

class C { v = 7; get() { return this.v; } }
function takesC(o: C) { console.log(o.get()); }
const f2: any = new C();
takesC(f2);

// the original decl stays untouched for typed callers — both spellings
// reach the same source fn
takesFn(function () { console.log("typed caller"); });

// two widen flavors on one call: an `any[]` arg (Arr target) beside an
// `any` arg (Scalar target)
function mix(xs: number[], g: () => void) { console.log(xs.length); g(); }
const seq: any[] = [];
seq.push(1);
seq.push(2);
const h: any = function () { console.log("mix fn"); };
mix(seq, h);

// the widened slot is not a silent pass: a non-callable behind `any`
// still throws where bun throws
const bad: any = 5;
try { takesFn(bad); } catch (err) { console.log("caught:", (err as Error).constructor.name); }

// return values flow on
function twice(g: (n: number) => number) { return g(g(1)); }
const inc: any = (n: number) => n + 10;
console.log(twice(inc));
