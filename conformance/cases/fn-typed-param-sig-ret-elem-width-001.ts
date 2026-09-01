// 553-01 leg 2 — the widened element class reaches the RESULT of a
// call made through a fn-typed param, not only the param's own read.
// The guarded edge tying `Ret(via)` to `Field(Param(via, "f"), "__ret")`
// was filed under a spelling the evidence never used: the argument site
// marks `Field(Field(Global(via), "__p0"), "__ret")`, and those two
// agree only through congruence, which runs after guarded-union
// activation. Asking for the evidence in deep-canonical form (rather
// than literally) lets the edge activate, so the array handed back out
// of `via` reads as the Arr(F64) it actually is. Before the fix this
// printed `4619567317775286272,4620693217682128896` — f64 bits read
// through an I64 slot.
//
// Keep this shape MINIMAL. Adding `p[0]` / `p.concat(...)` / a second
// caller supplies container evidence of its own, which activates the
// guarded edge under the literal query too and hides the bug: the
// same-day first draft of this fixture was green on the unfixed HEAD
// for exactly that reason.
const a = (n: number): number[] => [n, n + 1];
const boom = (): any => {
  throw new Error("x");
};
type Nums = number[];
type NumsFn = (n: number) => Nums;

const via = (f: NumsFn): number[] => f(7);

let p: number[] = [];
p = via(a);
console.log("param", p.join(","));

try {
  a(boom());
} catch (e) {
  console.log("c");
}
