// §13.15.5.3/4 — a yield in a destructuring-assignment default is
// CONDITIONAL: it evaluates (and suspends the generator) only when
// the slot answers undefined. The desugar recovers the hoisted
// YieldInto into a statement-level undefined-guard.
// (Generator DECLARATIONS here — the expression form still rejects on
// the RFC 20260713 reassigned-capture boundary, recorded in L3b.)
let x: any, y: any, r: any;
// array elem, slot absent → default fires → suspend, resume value binds
function* gen1() {
  r = [x = yield] = [];
  console.log("resumed", x, r.length);
}
const g1 = gen1();
console.log("n1", JSON.stringify(g1.next()));
console.log("n2", JSON.stringify(g1.next(86)));
// array elem, slot present → the default (and its yield) never runs
function* gen2() {
  [y = yield] = [7];
}
const g2 = gen2();
console.log("n3", JSON.stringify(g2.next()));
console.log("y", y);
// object field default
let f: any;
function* gen3() {
  ({ f = yield } = {});
  console.log("f-in", f);
}
const g3 = gen3();
g3.next();
console.log("n4", JSON.stringify(g3.next("F")));
// yield operands + a mixed present/absent pattern: only the absent
// slot's yield runs, in slot order
let a: any, b: any;
function* gen4() {
  [a = yield 1, b = yield 2] = [undefined, "B"];
}
const g4 = gen4();
console.log("n5", JSON.stringify(g4.next()));
console.log("n6", JSON.stringify(g4.next("A")));
console.log("a b", a, b);
console.log("done");
