// Chunk 641 — an empty `[]` literal argument admits any Array(T)
// param contextually (it has no element to infer from; bun accepts,
// tr rejected with "expected Array(Number), got Array(Any)"). The
// lowering pairs the checker admit with a param-typed empty alloc
// so the callee never sees a FLAG_ARR_ANY block behind a typed slot.
function take(xs: number[]): number {
  return xs.length;
}
console.log(take([]));
// ctor param
class N {
  xs: N[];
  constructor(seed: N[]) {
    this.xs = seed;
  }
}
const n = new N([]);
console.log(n.xs.length);
// the param-typed empty block accepts typed writes after the call
n.xs.push(n);
console.log(n.xs.length);
// FnSig-valued indirect call
const f: (xs: number[]) => number = take;
console.log(f([]));
// closure-valued call
function run(): void {
  const g = (xs: string[]): number => xs.length;
  console.log(g([]));
}
run();
// Arr<Any> param keeps taking the empty literal
function anyLen(xs: any[]): number {
  return xs.length;
}
console.log(anyLen([]));
console.log("done");
