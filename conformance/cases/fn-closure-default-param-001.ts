// r359 — a fn-typed param whose DEFAULT is closure-shaped: the
// call-site pad hands the cell into the param, which kept __fn(
// (the closure-param tag pass only scanned call ARGUMENTS) and
// dispatched the cell header as code (EXIT=138). Covers the
// closure-binding default, the closure-literal default, and the
// explicit-argument override.
const a = (x: number): number => x + 10;
function h(cb: (x: number) => number = a): number {
  return cb(5);
}
console.log(h());
console.log(h(a));
console.log(h((x: number): number => x * 2));
const off = 3;
function h2(k: number, cb: (x: number) => number = (x: number): number => x + off): number {
  return cb(k);
}
console.log(h2(7));
