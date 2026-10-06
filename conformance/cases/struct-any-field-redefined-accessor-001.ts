// A declared `any` field redefined as an accessor behaves as one: the
// pair must not land in the field's slot as its value.
class D { x: any = "a"; y: number = 2; }
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
class C { x: any = 1; }
const c: any = new C();
Object.defineProperty(c, "x", { get() { return "G"; }, configurable: true });
console.log(c.x, String(c.x).length);
