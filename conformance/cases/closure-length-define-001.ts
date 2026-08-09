// defineProperty'd own `length` / `name` on a function shadows the
// virtual fn metadata (§20.2.3 attributes are non-writable but
// CONFIGURABLE, so defineProperty is the sanctioned override door,
// and a partial descriptor completes against those attributes);
// deleting the virtual slot falls through to %Function.prototype%'s
// own `length` 0 / `name` "".
function f(a: any, b: any) {
  return a;
}
var g: any = f;
console.log(g.length, g.name);
Object.defineProperty(g, "length", { value: 7 });
console.log(g.length);
Object.defineProperty(g, "length", {
  get: function () {
    return 42;
  },
  configurable: true,
});
console.log(g.length);
Object.defineProperty(g, "name", { value: "renamed" });
console.log(g.name);
Object.defineProperty(g, "name", {
  get: function () {
    return "got";
  },
});
console.log(g.name);
function m(x: any, y: any, z: any) {
  return x;
}
var n: any = m;
var d: any = Object.getOwnPropertyDescriptor(n, "length");
console.log(d.value, d.writable, d.enumerable, d.configurable);
Object.defineProperty(n, "length", { value: 1 });
var d2: any = Object.getOwnPropertyDescriptor(n, "length");
console.log(d2.value, d2.writable, d2.enumerable, d2.configurable);
function h(x: any) {
  return x;
}
var k: any = h;
delete k.length;
delete k.name;
console.log(k.length, JSON.stringify(k.name));
Object.defineProperty(k, "length", { value: 9 });
console.log(k.length);
