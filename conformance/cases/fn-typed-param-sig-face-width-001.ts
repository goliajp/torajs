// 553-01 leg 1 — a fn-typed PARAM's embedded signature widens by its
// own key. `NumsFn` is an alias, so the slot's annotation is the bare
// alias name: it has no canonical `__fn(` spelling, the analysis never
// unioned it onto a nominal class, and the widen used to return the
// parse width unchanged. The argument actually passed has widened
// (the `a(boom())` any-argument is a legal f64 face on `a`), so `f(7)`
// read an Arr(F64) through the annotation's narrow Arr(I64) SigId.
const a = (n: number): number[] => [n, n + 1];
const boom = (): any => {
  throw new Error("x");
};
type Nums = number[];
type NumsFn = (n: number) => Nums;

const elem = (f: NumsFn): number => f(7)[0];
console.log("elem", elem(a));

const joined = (f: NumsFn): string => f(7).join("-");
console.log("join", joined(a));

const both = (f: NumsFn): void => {
  const r = f(7);
  console.log("inner", r[0], r[1]);
};
both(a);

try {
  a(boom());
} catch (e) {
  console.log("caught");
}
