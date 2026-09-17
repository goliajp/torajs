// K3 (RFC 20260917-class-eval-unit) — a class with private FIELDS is a
// unit class too: each evaluation gets its own class object, and a
// body that reaches #x refuses another evaluation's receiver
// (§7.3.31), while the field itself stays an ordinary layout slot.
function outer() {
  class C {
    #x = 1;
    read() { return this.#x }
    bump() { this.#x += 1; return this.#x }
    has(o: any) { return #x in o }
    peek(o: any) { return o.#x }
    poke(o: any) { o.#x = 7; return o.#x }
  }
  return C;
}
const C1 = outer(), C2 = outer();
const c1 = new C1(), c2 = new C2();
console.log(c1.read(), c2.read());
console.log(c1.bump(), c1.bump(), c2.read());
console.log(C1 === C2, C1.prototype === C2.prototype);
console.log(Object.getOwnPropertyNames(c1), JSON.stringify(c1));
console.log(c1.has(c1), c1.has({}));
console.log(C1.prototype.read.call(c1));
try { C1.prototype.read.call(c2); console.log("no throw") }
catch (e: any) { console.log("threw", e.constructor.name) }
// a receiver that is not `this` compares evaluations too: the other
// evaluation's instance carries the same row and is refused (§7.3.31),
// and `#x in o` answers false for it rather than throwing.
console.log(c1.peek(c1), c1.poke(c1), c1.has(c2));
try { c1.peek(c2); console.log("no throw") }
catch (e: any) { console.log("threw", e.constructor.name) }
try { c1.poke(c2); console.log("no throw") }
catch (e: any) { console.log("threw", e.constructor.name) }
