class P { #x = 1; y = 2; get x() { return this.#x; } }
const p: any = new P();
console.log(Object.getPrototypeOf(p) === P.prototype, p.x);
class Q extends P { #z = 3; }
const q: any = new Q();
console.log(Object.getPrototypeOf(q) === Q.prototype);
