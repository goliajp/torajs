// 553-02 — a fn type whose return is an array. The flat annotation
// spells the return in its own brackets (`__fn(P)->(R)`) so a trailing
// `[]` can only be the array wrapper; before that, every `[]` consumer
// stripped the suffix first and read `(n: number) => number[]` as an
// ARRAY of `(n: number) => number`.
const twice = (n: number): number[] => [n, n];

let g: (n: number) => number[] = twice;
console.log(g(4).join(","));

function apply(f: (n: number) => number[], k: number): number[] {
  return f(k);
}
console.log(apply(twice, 5).join(","));

type Maker = (n: number) => string[];
const m: Maker = (n: number): string[] => ["x".repeat(n)];
console.log(m(3).join("|"));

const holder: { f: (n: number) => number[] } = { f: twice };
console.log(holder.f(7).join(","));

function* gen(k: number): Generator<number> {
  const xs = twice(k);
  yield xs[0];
  yield xs[1];
}
for (const v of gen(3)) console.log(v);

// The other reading of the same tokens still works: an array OF fns
// keeps its `[]` outside the bracketed return.
const fs: (() => string)[] = [() => "a", () => "b"];
console.log(fs[0](), fs[1]());

// Nested: a fn returning a fn returning an array.
const outer = (a: number): ((b: number) => number[]) => (b: number) => [a, b];
console.log(outer(1)(2).join(","));
