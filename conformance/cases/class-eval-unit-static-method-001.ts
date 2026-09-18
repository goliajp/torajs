// K4-1 — a unit class (a class written in a function body) with static
// methods: every evaluation's class object carries its own function
// objects (§15.7.14), a static `this` is that evaluation's class
// object, and a direct `K.m()` inside the body reaches the same face.
function mk(): any {
  class K {
    static who() { return this }
    static tag(n: number) { return "k" + n }
    static twice(n: number) { return this.tag(n) + this.tag(n) }
    inst() { return "i" }
  }
  return K;
}
const A = mk(), B = mk();
console.log(A.who() === A, B.who() === B, A.who() === B, A.who !== B.who);
console.log(A.tag(1), B.twice(2), typeof A.tag, A.who.name, A.tag.length);
console.log(Object.getOwnPropertyNames(A).filter((n) => !["length", "name", "prototype"].includes(n)).join(","), new A().inst());
const w = A.who;
console.log(w.call(B) === B, w.call(null) === null);
console.log(Object.getOwnPropertyDescriptor(A, "tag")!.enumerable, A.hasOwnProperty("tag"), B.hasOwnProperty("tag"));
let s = 0;
for (let i = 0; i < 300; i++) { const Q = mk(); s += Q.tag(i).length; }
console.log(s);
