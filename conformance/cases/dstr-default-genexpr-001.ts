// RFC 20260729-fn-value-any V4 刀 1 — a destructuring-slot default
// holding a generator function EXPRESSION (hoisted to a
// `__genexpr_*` factory) wraps through its forwarder into the any
// slot instead of panicking the whole program at box_to_any; the
// bound value stays callable and drives the generator protocol.
// (A named generator DECLARATION as a destr default is a separate
// scope-aware axis — registered, not covered here.)
function f({ gen = function* () {
  yield 41;
} }: any) {
  console.log(typeof gen);
  const it: any = gen();
  console.log(it.next().value);
}
f({});
function g({ pick = function* named() {
  yield 7;
} }: any) {
  console.log(typeof pick);
  const it: any = pick();
  console.log(it.next().value);
}
g({});
