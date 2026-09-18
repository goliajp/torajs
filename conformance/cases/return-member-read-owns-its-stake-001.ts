// A function returning a field read (`return this.s`, `return p.name`,
// `return p.a.b.c`, a getter doing the same) must hand the caller its own
// stake. It used to hand back the FIELD'S: dropping the result freed the
// field's cell under the live object, and the next same-size allocation
// read back through it (`p.name` answered the later string).
class C { s: string; inner: { v: string }; constructor(i: number) { this.s = "cd" + i; this.inner = { v: "in" + i }; } get(): string { return this.s; } get v(): string { return this.s; } pick(): { v: string } { return this.inner; } }
function nameOf(p: { name: string }): string { return p.name; }
function deep(p: { a: { b: { c: string } } }): string { return p.a.b.c; }
function run(i: number): void {
  const p = { name: "ab" + i }; { const r = nameOf(p); } const t = "xy" + i; console.log(p.name, t);
  const c = new C(i); { const r = c.get(); } const t2 = "zw" + i; console.log(c.s, t2);
  { const r = c.v; } const t3 = "qq" + i; console.log(c.s, c.v, t3);
  { const r = c.pick(); } const t4 = { v: "ot" + i }; console.log(c.inner.v, t4.v);
  const d = { a: { b: { c: "deep" + i } } }; { const r = deep(d); } const t5 = "zzzz" + i; console.log(d.a.b.c, t5);
}
for (let i = 1; i <= 3; i++) run(i);
