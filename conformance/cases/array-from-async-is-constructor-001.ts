// Array.fromAsync with a callable NON-constructor as |this|: spec
// IsConstructor (proposal §2.1.1 steps 3.a / 7.a) says such a C is
// not constructed and the result is a plain array. The self-hosted
// helper spelled IsConstructor as `typeof c === "function"`, which
// `new`ed an arrow (a TypeError) and a builtin static; it now asks
// the runtime's own IsConstructor. (A bound class as |this| answers
// true there too, but `new` on a bound function through the any lane
// is a separate, older hole — registered, not exercised here.)
class Coll { n: number; constructor(n?: number) { this.n = n === undefined ? -1 : n; } }
async function main(): Promise<void> {
  const arrow: any = () => 1;
  const a = await (Array.fromAsync as any).call(arrow, [1, 2]);
  console.log("arrow", Array.isArray(a), a.join(","));
  const b = await (Array.fromAsync as any).call(Math.max, { length: 2, 0: "x", 1: "y" });
  console.log("static", Array.isArray(b), b.join(","));
  const c = await (Array.fromAsync as any).call(Coll, [3, 4]);
  console.log("ctor", c instanceof Coll, c.n, c[0], c[1]);
}
main();
