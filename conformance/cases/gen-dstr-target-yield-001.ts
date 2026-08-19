// §13.15.5.3 step 1 — a yield inside a destructuring-assignment
// TARGET reference evaluates at its own slot (the generator suspends
// there), not hoisted in front of the whole statement.
const x: any = {};
function* g1() {
  [x[yield]] = [86];
}
const i1 = g1();
console.log(JSON.stringify(i1.next()));
console.log(JSON.stringify(i1.next("p")), x.p);
// rest-nested-array form
const y: any = {};
function* g2() {
  [...[y[yield]]] = [23];
}
const i2 = g2();
i2.next();
console.log(JSON.stringify(i2.next("q")), y.q);
// object-property form
const z: any = {};
function* g3() {
  ({ v: z[yield] } = { v: 7 });
}
const i3 = g3();
i3.next();
console.log(JSON.stringify(i3.next("r")), z.r);
// nested-array pattern in a for-of head (the r453 sweep regression
// pair): the nested src temp crosses the suspension and lifts to a
// state-machine field — its annotation must stay indexable.
const w: any = {};
function* g4() {
  for ([[w[yield]]] of [[[22]]]) {
  }
}
const i4 = g4();
i4.next();
console.log(JSON.stringify(i4.next("s")), w.s);
console.log("done");
