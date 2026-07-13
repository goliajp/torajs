// RFC 20260713-generator-fn-value-substrate blade 5 (cut 1) —
// generator fn `.prototype` reflection: g.prototype is the same
// object Object.getPrototypeOf(g()) answers (across the Obj and Any
// tiers), unique per generator fn, and `inst instanceof g` walks it
// per §27.5.3 [[HasInstance]].

function* g() {
  yield 1;
}
function* g2() {
  yield 2;
}

const gp = g.prototype;
console.log("typeof:", typeof gp);

const inst: any = g();
console.log("proto match (any tier):", Object.getPrototypeOf(inst) === gp);

const typedInst = g();
console.log("proto match (typed tier):", Object.getPrototypeOf(typedInst) === gp);

console.log("instanceof:", inst instanceof g);
console.log("not instanceof g2:", !(inst instanceof g2));
console.log("uniqueness:", gp !== g2.prototype);
