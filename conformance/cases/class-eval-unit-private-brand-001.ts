// RFC 20260917-class-eval-unit K2 — a private method or accessor of a
// class declared in a function belongs to one evaluation of it.
function makeM(): any {
  class C {
    #m(): string { return "m"; }
    get #g(): string { return "g"; }
    set #s(v: string) { this.last = v; }
    last = "";
    callM(o: any): string { return o.#m(); }
    readG(o: any): string { return o.#g; }
    writeS(o: any): string { o.#s = "w"; return o.last; }
  }
  return new C();
}
const a = makeM();
const b = makeM();
console.log(a.callM(a), a.readG(a), a.writeS(a));
for (const [label, f] of [
  ["method", () => a.callM(b)],
  ["getter", () => a.readG(b)],
  ["setter", () => b.writeS(a)],
] as [string, () => string][]) {
  try { f(); console.log(label, "no throw"); } catch (e) { console.log(label, e instanceof TypeError); }
}
function typed(): string {
  class T { #v(): number { return 7; } same(o: T): number { return o.#v(); } }
  const t1 = new T();
  const t2 = new T();
  return String(t1.same(t2));
}
console.log(typed());
