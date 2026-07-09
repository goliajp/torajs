// L3b #4 — a shorter-arity function value fits a wider fn-typed slot
// at EVERY assignment position (TS lets callbacks ignore trailing
// params); extra call args are evaluated and simply never read.
// let position
const pick: (a: number, b: number) => number = (x: number): number => x;
console.log(pick(7, 8));
// zero-arity into a two-param slot
const answer: (a: number, b: number) => number = (): number => 42;
console.log(answer(1, 2));
// call-arg position (the S133 lattice, regression face)
function apply(f: (a: number, b: number) => number): number {
  return f(3, 4);
}
console.log(apply((x: number): number => x * 2));
// annotated mutable local: same-arity + shrink reassign (the slot
// re-reprs Closure so env-carrying arrows can land)
function swapLocal(): number {
  let cb: (a: number, b: number) => number = (a: number, b: number): number => a + b;
  let t = cb(5, 6);
  cb = (a: number, b: number): number => a * b;
  t = t + cb(5, 6);
  cb = (x: number): number => x;
  return t + cb(5, 6);
}
console.log(swapLocal());
// un-annotated mutable local shrink reassign
let cb2 = (a: number, b: number): number => a + b;
cb2 = (x: number): number => x;
console.log(cb2(5, 6));
// promoted global slot with a shrink init, read from a named fn
const gpick: (a: number, b: number) => number = (x: number): number => x + 100;
function useGpick(): number {
  return gpick(9, 10);
}
console.log(useGpick());
// struct-field position
type Ops = { f: (a: number, b: number) => number };
const o: Ops = { f: (x: number): number => x + 1 };
console.log(o.f(5, 9));
console.log("done");
