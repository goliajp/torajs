// K4-4 — static private members of a class evaluated more than once
// (§15.7.14): each evaluation owns its private names, so a static
// body of one evaluation run against the OTHER evaluation's class
// object is a brand failure (§7.3.31), while each answers for itself.
function mkMethod(): any {
  class K {
    static #m() { return "x"; }
    static access() { return this.#m(); }
    static pub() { return 1; }
  }
  return K;
}
function mkAccessor(): any {
  class K {
    static get #g() { return "g"; }
    static set #g(v: any) { (this as any).s = v; }
    static rd() { return this.#g; }
    static wr(v: any) { this.#g = v; }
  }
  return K;
}
function mkField(): any {
  class K {
    static #f = 1;
    static rd() { return this.#f; }
    static wr(v: any) { this.#f = v; }
    static has(o: any) { return #f in o; }
  }
  return K;
}
function thrown(f: () => void): string {
  try { f(); return "no throw"; } catch (e) { return (e as any).constructor === TypeError ? "TypeError" : "other"; }
}
{
  const A = mkMethod(), B = mkMethod();
  console.log(A === B, A.access(), B.access(), A.access === B.access);
  console.log(thrown(() => A.access.call(B)), thrown(() => B.access.call(A)));
  console.log(thrown(() => A.access.call(1)), thrown(() => A.access.call(new A())));
  console.log(thrown(() => A.access.apply(B, [])), A.access.call(A), A.pub.call(B));
  const o: any = { f: A.access };
  console.log(thrown(() => o.f()));
  console.log(Object.getOwnPropertyNames(A).join(","));
}
{
  const A = mkAccessor(), B = mkAccessor();
  console.log(A.rd(), B.rd());
  A.wr(3);
  console.log(A.s, B.s);
  console.log(thrown(() => A.rd.call(B)), thrown(() => A.wr.call(B, 1)), B.s);
}
{
  const A = mkField(), B = mkField();
  A.wr(9);
  console.log(A.rd(), B.rd(), A.has(A), A.has({}));
  console.log(thrown(() => A.rd.call(B)), thrown(() => A.wr.call(B, 1)), B.rd());
  console.log(Object.getOwnPropertyNames(A).join(","));
}
