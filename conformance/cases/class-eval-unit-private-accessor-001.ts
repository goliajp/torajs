// RFC 20260917-class-eval-unit K2b, the accessor half — a unit class
// with a private getter / setter: each evaluation's accessor faces are
// its own function objects (§15.7.14), mortal and released with the
// evaluation's prototype, where they used to be minted immortal on
// every evaluation. Behaviour is the same on both sides; the fixture
// pins that the faces still work and that the evaluations stay apart.
function outer() {
  class C {
    #v = 10;
    get #x() { return this.#v }
    set #x(n: number) { this.#v = n }
    bump() { this.#x = this.#x + 1; return this.#x }
    peek() { return this.#x }
  }
  return C;
}
const C1 = outer(), C2 = outer();
const c1 = new C1(), c2 = new C2();
console.log(c1.bump(), c1.bump(), c2.bump(), c1.peek(), c2.peek());
console.log(C1 === C2, C1.prototype === C2.prototype, c1 instanceof C1, c1 instanceof C2);
console.log(Object.getOwnPropertyNames(C1.prototype).join(","));
try { C1.prototype.peek.call(c2); console.log("no throw") }
catch (e: any) { console.log("threw", e.constructor.name) }
let total = 0;
for (let i = 0; i < 2000; i++) { const K = outer(); total += new K().bump(); }
console.log(total);
