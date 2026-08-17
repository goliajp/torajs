// 423-03 ④ — a fn-typed LET slot admits the same shorter-face /
// widened-prefix function a call argument does, and its mismatched
// head-less init reabstracts through a sig-exact thunk: `const
// slot: () => void = gb` used to be a checker reject, and without
// the thunk the slot's narrower call_indirect read garbage.
function mk(): number {
  console.log("default evaluated");
  return 42;
}
function gb(p = mk()) {
  console.log("gb sees:", p);
}
const slot: () => void = gb;
slot();
const slot2: (p: number) => void = gb;
slot2(7);
function nested() {
  const inner: () => void = gb;
  inner();
}
nested();
