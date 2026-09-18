// K5-2 — private elements across `extends` inside a per-evaluation
// family: the brand of A's elements is "evaluated under THIS A", which
// a B instance answers through its class's recorded ancestors.
function mk(): any {
  class A {
    #x = 1;
    #m() { return "m" + this.#x }
    static same(p: any, q: any) { return p.#x + q.#x }
    getX() { return this.#x }
    peek(o: any) { return o.#x }
    callM(o: any) { return o.#m() }
    has(o: any) { return #x in o }
  }
  class B extends A {
    #y = 10;
    sum(o: any) { return o.#y + this.getX() }
    hasY(o: any) { return #y in o }
  }
  class C extends B {
    #z = 100;
    all() { return this.#z + this.sum(this) }
  }
  return [A, B, C]
}
const [A1, B1, C1] = mk(), [A2, B2, C2] = mk();
const t = (f: () => any) => {
  try { console.log(f()) } catch (e) { console.log("T", (e as any).constructor === TypeError) }
};
console.log(A1.same(new A1(), new B1()), new B1().getX(), A1.prototype.getX.call(new B1()));
console.log(new C1().all(), new C1().getX(), A1.same(new C1(), new B1()));
t(() => A1.same(new A1(), new B2()));
t(() => A1.prototype.getX.call(new B2()));
t(() => A1.prototype.getX.call(new C2()));
t(() => B1.prototype.getX.call(new C1()));
t(() => new A1().peek(new B1()));
t(() => new B1().peek(new A1()));
t(() => new B1().peek(new B2()));
t(() => new C1().peek(new A2()));
t(() => new B1().callM(new A1()));
t(() => new A1().callM(new C1()));
t(() => new A1().callM(new B2()));
t(() => new B1().has(new A1()));
t(() => new B1().has(new A2()));
t(() => new A1().has(new C2()));
t(() => new B1().sum(new C1()));
t(() => new B1().sum(new A1()));
t(() => new B1().sum(new C2()));
t(() => new C1().hasY(new B1()));
t(() => new C1().hasY(new A1()));
t(() => new C1().hasY(new B2()));
