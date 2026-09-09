// The any-lane operator kernels (any_add / any_arith / any_bitwise /
// any_compare) record a TypeError for a mixed BigInt pair, a Symbol
// operand, or a throwing valueOf, and the lowering checks right after
// the call — but a named fn whose only raise is such an operator was
// not in the caller's may-throw set, so the caller skipped its check:
// `f(3)` below printed the propagate sentinel ([unknown-any-tag]) and
// the TypeError surfaced statements later, uncaught.
function mulC(a: any) { return a * 2n; }
function subN(a: any, b: any) { return a - b; }
function addA(a: any, b: any) { return a + b; }
function cmp(a: any, b: any) { return a < b; }
function bit(a: any, b: any) { return a & b; }
function after(a: any) { const r = a * 2n; console.log("after must not print"); return r; }
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", f()); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name); }
}
t("bigint*bigint", () => mulC(3n));
t("number*bigint", () => mulC(3));
t("bigint-bigint", () => subN(7n, 2n));
t("number-bigint", () => subN(7, 2n));
t("bigint+number", () => addA(1n, 1));
t("string+bigint", () => addA("a", 1n));
t("bigint<number", () => cmp(1n, 2));
t("bigint&number", () => bit(3n, 1));
t("symbol*2", () => subN(Symbol("s"), 2));
t("throwing valueOf", () => subN({ valueOf() { throw new RangeError("no"); } }, 1));
t("after", () => after(3));
console.log("end");
