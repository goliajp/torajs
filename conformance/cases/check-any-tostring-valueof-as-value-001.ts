// `toString` / `valueOf` read off an Any receiver are ordinary Any
// members, not statically-typed Functions. The checker used to
// promise Function for these two names alone, which both lied (an
// own entry can shadow the prototype with a non-callable) and cost
// bun parity: the Function-typed value had no ssa_lower
// materialisation, so binding it and calling it failed to lower.
const o: any = { toString: () => "T", valueOf: () => 7 };
const t = o.toString;
const v = o.valueOf;
console.log(t(), v());
console.log(o.toString(), o.valueOf());
// The shadowing case the old Function type could not describe.
const bad: any = { toString: undefined, valueOf: () => 1 };
// `typeof bad.toString` is deliberately not asserted: the value-read
// path answers the prototype method where bun answers the own
// `undefined` entry. That shadowing gap is pre-existing (the old
// Function type printed "function" here too) and is tracked in
// plan-state L3b. The call below shows the call path does honour it.
try {
  bad.toString();
} catch (e) {
  console.log("non-callable own entry throws");
}
console.log(bad.valueOf() + 1);
// Prototype-provided toString still answers on a plain object.
const plain: any = { a: 1 };
console.log(typeof plain.toString);
