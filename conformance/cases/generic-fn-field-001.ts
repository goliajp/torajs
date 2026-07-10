// chunk 795 — fn-type args of a generic struct instantiation are
// Closure-repr fields. Three fronts, all `->`-arrow or retag holes:
// the checker/SSA generic-arg splitters counted the arrow's `>` as
// a generic closer (`Pair<() => number, string>` split into one
// bogus arg — "unknown type" reject); the SSA instantiation
// substituted `__fn(` into the field layout (FnSig slot vs closure
// env block stored — SIGBUS on the field call); and the named-fn
// forwarder wrap axes couldn't resolve `Box<() => number>`
// annotations at all.
type Box<T> = { v: T };
const b: Box<() => number> = { v: () => 5 };
console.log(b.v());

type Pair<A, B> = { a: A; b: B };
const p: Pair<() => number, string> = { a: () => 6, b: "x" };
console.log(p.a());
console.log(p.b);

function topfn(): number {
  return 33;
}
function two(): number {
  return 55;
}
const g: Box<() => number> = { v: topfn };
console.log(g.v());
g.v = () => 44;
console.log(g.v());
g.v = two;
console.log(g.v());

// control: non-fn generic instantiation
const n: Box<number> = { v: 9 };
console.log(n.v);
