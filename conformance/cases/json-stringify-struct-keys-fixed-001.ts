// §25.5.2.4 SerializeJSONObject takes K = EnumerableOwnProperties
// before it reads any value: a key the replacer adds mid-walk stays
// out.
class D {
  b = 2;
  c = 1;
}
const d: any = new D();
d.x = 1;
console.log(JSON.stringify(d, function (key, v) {
  if (key === "b") d.z = 9;
  return v;
}));
console.log(JSON.stringify(d));
const d2: any = new D();
console.log(JSON.stringify(d2, function (key, v) {
  if (key === "b") d2.z = [9];
  return v;
}));
console.log(JSON.stringify(d2));
const k = String(process.argv.length > 100 ? "q" : "kk");
class E {
  b = 2;
  [k] = 1;
}
const e: any = new E();
console.log(JSON.stringify(e, function (key, v) {
  if (key === "b") e.z = 9;
  return v;
}));
// a key a member's toJSON adds to the holder mid-walk stays out too
class G {
  b = 2;
  c: any = 1;
}
const g: any = new G();
g.x = 1;
g.c = {
  toJSON() {
    g.w = 3;
    return 7;
  },
};
console.log(JSON.stringify(g));
console.log(JSON.stringify(g));
// an accessor `defineProperty` puts on an instance serializes through
// its getter, the instance as its receiver
class F {
  b = 2;
}
const f = new F();
Object.defineProperty(f, "g", {
  get() {
    return this.b + 1;
  },
  enumerable: true,
});
console.log(JSON.stringify(f));
