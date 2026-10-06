// a literal spelled in another order than its declared struct type,
// in call lanes that do and do not name the parameter's layout
type A = { b: string; x: number };
class K {
  m(p: A): string {
    return p.b + p.x;
  }
}
console.log(new K().m({ x: 1, b: "m" }));
class C2 {
  a: A;
  constructor(p: A) {
    this.a = p;
  }
}
console.log(JSON.stringify(new C2({ x: 2, b: "c" }).a));
const fnv = (p: A): string => p.b + p.x + Object.keys(p).join(",");
console.log(fnv({ x: 3, b: "f" }));
function dflt(p: A = { x: 1, b: "d" }): string {
  return p.b + JSON.stringify(p);
}
console.log(dflt());
class H {
  a: A = { x: 1, b: "h" };
}
console.log(new H().a.b, JSON.stringify(new H().a));
const xs: A[] = [];
xs.push({ x: 1, b: "p" });
console.log(xs[0].b, JSON.stringify(xs));
const m = new Map<string, A>();
m.set("k", { x: 1, b: "mp" });
console.log(m.get("k")!.b, JSON.stringify(m.get("k")));
// a repeated key keeps its first position
const e: A = { x: 1, b: "q", x: 3 };
console.log(e.b, e.x, JSON.stringify(e), Object.keys(e).join(","));
// spread and rest copy in the source's creation order
type W = { a: A; n: number };
const w: W = { n: 1, a: { x: 2, b: "z" } };
console.log(w.a.b, JSON.stringify(w));
console.log(JSON.stringify({ ...w.a }), JSON.stringify({ y: 0, ...w.a }));
const { x, ...rest } = w.a;
console.log(x, JSON.stringify(rest));
// through the any lane and back into a typed parameter
function g(p: A): A {
  return p;
}
const z: any = w.a;
console.log(z.b, z.x, JSON.stringify(g(z)), Object.keys(g(z)).join(","));
// a cast literal is stored in the cast type's layout
const v = { x: 1, b: "as" } as A;
console.log(v.b, v.x, JSON.stringify(v));
const v2: A = { x: 2, b: "as2" } as A;
console.log(v2.b, v2.x, Object.keys(v2).join(","));
async function am(): Promise<A> {
  return { x: 5, b: "am" };
}
am().then((r) => console.log(r.b, r.x, JSON.stringify(r)));
