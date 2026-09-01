// 553-03 — a `__fn(`-annotated return whose value reaches the return
// through a value-passthrough operator. The closure-repr marking pass
// only recognised a closure literal, a closure-holding ident, a call
// to a ret-marked fn, and member/index reads, so `k > 0 ? f1 : f2`
// left the annotation on the bare-fn-pointer lane while the value
// arrived as a closure cell: calling the result `blr`'d the cell's
// heap header (EXIT=138). The alias spelling (`type Thunk = () =>
// number`) was always right — it never enters this lane.
const f1 = (): number => 1;
const f2 = (): number => 2;

const ternary = (k: number): () => number => (k > 0 ? f1 : f2);
console.log(ternary(1)(), ternary(-1)());

function ternaryDecl(k: number): () => number {
  return k > 0 ? f1 : f2;
}
console.log(ternaryDecl(1)(), ternaryDecl(-1)());

// the comma operator and a type assertion hand the operand straight
// through the same way
const comma = (k: number): () => number => (k, f1);
console.log(comma(9)());

const cast = (k: number): () => number => (k > 0 ? f1 : f2) as () => number;
console.log(cast(1)(), cast(-1)());

// nested: the passthrough chains
const nested = (k: number): () => number => (k > 0 ? (k, f1) : f2);
console.log(nested(1)(), nested(-1)());
