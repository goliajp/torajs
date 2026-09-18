// K4-3 — a unit class with static accessors: each evaluation's class
// object carries its own getter / setter function objects, `this`
// inside them is that evaluation's class object, and a direct `C.v`
// inside the body reaches the same pair.
function mk(): any {
  class K {
    static k = 1;
    static get v() { return this.k * 10 }
    static set v(x: number) { this.k = x }
    static get ro() { return "ro" }
    static bump() { this.v = this.v / 10 + 1; return this.v }
  }
  return K;
}
const A = mk(), B = mk();
console.log(A.v, B.v, A.ro);
A.v = 5;
console.log(A.v, B.v, A.k, B.k, A.bump(), B.bump());
const da = Object.getOwnPropertyDescriptor(A, "v")!, db = Object.getOwnPropertyDescriptor(B, "v")!;
console.log(typeof da.get, typeof da.set, da.get === db.get, da.get!.call(B), da.set!.length, da.get!.length, da.enumerable, da.configurable);
console.log(Object.getOwnPropertyDescriptor(A, "ro")!.set, A.ro, Object.keys(A).join(","));
let s = 0;
for (let i = 0; i < 200; i++) { const Q = mk(); Q.v = i; s += Q.v; }
console.log(s);
