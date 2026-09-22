// §10.1.6.3 ValidateAndApplyPropertyDescriptor turns a configurable
// data property into an accessor. A declared class field redefined
// with a getter kept the AccessorPair as a data value, so every read
// answered the pair itself (`[object]`).
class C {
  x: string = "a";
  n: number = 1;
}
const c: any = new C();
Object.defineProperty(c, "x", { get() { return "G"; }, configurable: true });
let calls = 0;
Object.defineProperty(c, "n", { get() { calls++; return this.x + "!"; } });
console.log(c.x, c.n, calls, String(c.x).length);
const e: any = new Error("m");
Object.defineProperty(e, "name", { get() { return "Named"; } });
console.log(e.name, String(e));

// The write runs the setter; the other own-property faces see an
// accessor, not the layout slot.
class D { x: string = "a"; y: number = 2; }
const d: any = new D();
let seen = "";
Object.defineProperty(d, "x", {
  get() { return "G"; },
  set(v: any) { seen = String(v); },
  enumerable: true,
  configurable: true,
});
d.x = "w";
const desc = Object.getOwnPropertyDescriptor(d, "x")!;
console.log(seen, d.x, Object.keys(d).join(), JSON.stringify(d), typeof desc.get, typeof desc.set, "value" in desc);
