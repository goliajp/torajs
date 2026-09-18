// K6-1 — the outer class is itself a unit class: the member bodies it
// took along when hoisted are still scanned for class expressions.
function mk(): any { class Outer { make(): any { return class { tag(): string { return "t"; } }; } } return Outer; }
const O1: any = mk(), O2: any = mk();
console.log(O1 === O2);
const I1: any = new O1().make(), I2: any = new O1().make(), I3: any = new O2().make();
console.log(I1 === I2, I1 === I3);
console.log(new I1() instanceof I1, new I1() instanceof I3);
console.log(new I3().tag());
