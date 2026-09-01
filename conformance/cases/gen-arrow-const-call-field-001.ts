// rotation 553 — a generator local initialized by a call through a
// top-level arrow-const takes the arrow's DECLARED return type as its
// lifted field annotation (551-02). The declared-return collector
// only scanned FnDecls, so the call declined and the field took the
// number fallback: `const t = s(k)` across a yield was "type mismatch
// assigning to t: field is Number, value is String". Same
// declared-not-inferred rule as FnDecls: an unannotated arrow's
// result field is `any`.
const s = (n: number): string => "v" + n;
// Alias spelling — an arrow-const whose declared return is spelled
// `number[]` hits 553-02 when the generator captures it (the fn-value
// field annotation flattens to `__fn(number)->number[]` and the
// decoder strips the trailing `[]` first, "not callable:
// Array(Function(...))"; pre-existing on HEAD).
type Nums = number[];
const twice = (n: number): Nums => [n, n];
const loose = (n: number) => n + 1;
function named(n: number): string {
  return "n" + n;
}

function* g(k: number): Generator<string> {
  const t = s(k);
  yield "a";
  yield t;
}
const out: string[] = [];
for (const v of g(7)) {
  out.push(v);
}
console.log(out.join(","));

function* h(k: number): Generator<number> {
  const xs = twice(k);
  yield xs[0];
  const m = loose(k);
  yield m;
  const nm = named(k);
  yield nm.length;
}
// (Collected by string concat: pushing the for-of value into a
// `number[]` still trips the container width analysis on the
// generator-step seam — 553-04, unlocked by this knife, HEAD never
// got past the field-type error.)
let acc = "";
for (const v of h(3)) {
  acc = acc + v + ",";
}
console.log(acc);

// The throw face the original repro churned on: the callee result
// held across a yield stays a String field while later iterations
// throw.
const boom = (): any => {
  throw new Error("x");
};
function* gb(k: number): Generator<string> {
  const t = s(k);
  yield t;
  yield boom();
}
let caught = 0;
for (let i = 0; i < 200; i++) {
  try {
    for (const x of gb(i)) {
    }
  } catch (e) {
    caught++;
  }
}
console.log(caught);
