// A unit class's STATIC body reading an instance private element: the
// running evaluation is the static `this`, so an instance of the same
// evaluation passes the brand and another evaluation's throws.
function mk(): any {
  class A {
    #x = 1;
    #m() { return 5 }
    static same(p: any, q: any) { return p.#x + q.#x }
    static callM(o: any) { return o.#m() }
    static has(o: any) { return #x in o }
    static viaArrow(o: any) { return [o].map((v: any) => v.#x)[0] }
    static write(o: any, v: number) { o.#x = v; return o.#x }
    peek(o: any) { return o.#x }
  }
  return A
}
const A1 = mk(), A2 = mk();
const t = (f: () => any) => {
  try { console.log(f()) } catch (e) { console.log("T", (e as any).constructor === TypeError) }
};
t(() => A1.same(new A1(), new A1()));
t(() => A1.same(new A1(), new A2()));
t(() => A1.callM(new A1()));
t(() => A1.callM(new A2()));
t(() => A1.has(new A1()));
t(() => A1.has(new A2()));
t(() => A1.has({}));
t(() => A1.viaArrow(new A1()));
t(() => A1.viaArrow(new A2()));
t(() => A1.write(new A1(), 9));
t(() => A1.write(new A2(), 9));
t(() => new A1().peek(new A1()));
t(() => new A1().peek(new A2()));
