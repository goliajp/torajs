// §7.3.14 Call(GetV(p, "m"), p) — a user function reached through a
// trap-less Proxy runs with the proxy as `this`; builtins keep working.
class A {
  #x = 10;
  static #s = 1;
  g() { return this.#x; }
  static s() { return (this as any).#s; }
  who() { return this; }
}
const a = new A();
const pa: any = new Proxy(a, {});
const pc: any = new Proxy(A, {});
const obj = { m(this: any) { return this; }, n: 3 };
const po: any = new Proxy(obj, {});
const arr: any = new Proxy([1, 2, 3], {});
const probes: [string, () => unknown][] = [
  ["instance direct", () => a.g()],
  ["instance via proxy", () => pa.g()],
  ["static via proxy", () => pc.s()],
  ["class method this is proxy", () => pa.who() === pa],
  ["object method this is proxy", () => po.m() === po],
  ["array join", () => arr.join("-")],
  ["array length", () => arr.length],
];
for (const [name, f] of probes) {
  try {
    console.log(name, "->", f());
  } catch (e) {
    console.log(name, "throws", (e as any) instanceof TypeError);
  }
}
