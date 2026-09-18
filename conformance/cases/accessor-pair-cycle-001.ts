// An accessor's getter and setter are owned by the AccessorPair the
// property stores. When they close over the object that carries the
// property, obj -> pair -> closure -> obj is a ring the collector has
// to walk through the pair; before it did, every such object leaked
// and so did every evaluation of a unit class with a private accessor.
function ring(n: number): number {
  const o: any = { n };
  Object.defineProperty(o, "twice", {
    get() { return o.n * 2; },
    set(v: number) { o.n = v; },
    configurable: true,
  });
  o.twice = n + 1;
  return o.twice;
}
let s = 0;
for (let i = 0; i < 2000; i++) s += ring(i);
console.log(s);

function outer() {
  class K {
    #v = 1;
    get #x() { return this.#v; }
    set #x(n: number) { this.#v = n; }
    bump() { this.#x = this.#x + 1; return this.#x; }
  }
  return K;
}
let t = 0;
for (let i = 0; i < 2000; i++) { const Q = outer(); t += new Q().bump(); }
console.log(t, outer() === outer());

// A getter pulled out of its descriptor outlives the object and the
// pair that minted it.
function keep(): any {
  const o: any = {};
  Object.defineProperty(o, "g", { get() { return 42; }, configurable: true });
  const d: any = Object.getOwnPropertyDescriptor(o, "g");
  return d.get;
}
const g = keep();
console.log(g(), typeof g);

// Getter only, setter only, and neither — the pair walks whatever is
// present and skips the absent half.
const h: any = {};
Object.defineProperty(h, "a", { get() { return h.k; }, configurable: true });
Object.defineProperty(h, "b", { set(v: number) { h.k = v; }, configurable: true });
h.b = 9;
console.log(h.a, h.b, Object.keys(h).join(","));
