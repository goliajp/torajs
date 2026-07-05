// RFC 20260705 chunk 553 — closure values through `__fn(`-annotated
// return lanes: param passthrough (keep), closure-literal return
// (make), named-fn-only return (choose, stays direct), mixed return
// (chooseMix, forwarder wrap), ret-marked call result flowing into a
// downstream fn-typed param (useIt), and mixed args at one param
// (pick). Pre-553 `held(7)` blr'd the env heap header (SIGBUS) and
// the arg-temp release finalized a shared env without an rc gate
// (UAF alias after the next alloc).
function keep(f: (n: number) => number): (n: number) => number {
  return f;
}
let held = keep((n: number) => n * 3);
console.log(held(7));

function make(): (n: number) => number {
  return (n: number) => n + 10;
}
let m = make();
console.log(m(5));

function double(n: number): number {
  return n * 2;
}
function choose(): (n: number) => number {
  return double;
}
let c = choose();
console.log(c(3));

function chooseMix(flag: boolean): (n: number) => number {
  if (flag) {
    return (n: number) => n + 100;
  }
  return double;
}
console.log(chooseMix(true)(1));
console.log(chooseMix(false)(1));

function useIt(g: (n: number) => number): number {
  return g(9);
}
console.log(useIt(held));

function pick(f: (n: number) => number): (n: number) => number {
  return f;
}
let viaClosure = pick((n: number) => n - 1);
console.log(viaClosure(6));
let viaNamed = pick(double);
console.log(viaNamed(6));
