// only one face overridden: the subclass's own accessor property
// shadows the whole pair, so the missing face is absent
class B {
  _v = 10;
  get v(): number { return this._v; }
  set v(x: number) { this._v = x; }
}
class G extends B {
  get v(): number { return this._v + 7; }
}
class S extends B {
  set v(x: number) { this._v = x * 2; }
}
function show(b: B): any { return b.v; }
function put(b: B, x: number): string {
  try {
    b.v = x;
    return "ok " + b._v;
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log("get-only", show(new G()), put(new G(), 1));
console.log("set-only", show(new S()), put(new S(), 4));
console.log("base", show(new B()), put(new B(), 4));
const g = new G();
console.log("static-g", g.v);
try {
  g.v = 3;
  console.log("static-g-set", g._v);
} catch (e) {
  console.log("static-g-set", (e as Error).constructor.name, g._v);
}
const s = new S();
s.v = 9;
console.log("static-s", s._v, s.v);
const items: B[] = [new B(), new G(), new S()];
console.log("mixed", items.map((b) => String(b.v)).join(","));
