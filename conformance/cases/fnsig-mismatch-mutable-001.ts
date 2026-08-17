// 424-04 — a MUTABLE fn-typed binding holding a face-mismatched fn
// (declared params past the annotation's arity, the S2 excess-Any
// admit) dispatches through the boxed dual entry: argc + the
// undefined-filled argv make the callee's materialized default
// guards fire per §10.2.1.4, where the bare annotation-shaped
// call_indirect read argument registers the caller never filled.
// Covers init and assign stores, expression and literal defaults, a
// value-returning face, a passed-prefix face, and the nested-fn
// flavor (materialize Phase C: a nested FnDecl's defaults guard in
// the body now — no dynamic call can reach the call-site pad).
function mk(): number {
  return 42;
}
function noop(): void {}
function gb(p = mk()): void {
  console.log(p);
}
let slot: () => void = noop;
slot();
slot = gb;
slot();

function gr(p = 5): number {
  return p * 2;
}
let fslot: () => number = gr;
console.log(fslot());
function alt(x = 9): number {
  return x;
}
fslot = alt;
console.log(fslot());

function g2(a: number, b = 100): number {
  return a + b;
}
let g2slot: (a: number) => number = g2;
console.log(g2slot(1));
console.log(g2slot(7));

function inner(): void {
  function lg(p = 33): void {
    console.log("local:", p);
  }
  let ls: () => void = lg;
  ls();
  ls = lg;
  ls();
  const cs: () => void = lg;
  cs();
}
inner();
