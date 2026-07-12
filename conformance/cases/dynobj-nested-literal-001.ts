// nested ObjectLit inside an any-dynobj literal recurses through the
// dynobj lane (RFC 20260712-object-create-define-props) — the inner
// object is dynobj-backed, so reflection and runtime descriptor walks
// work on it
const x: any = { a: { b: 1, c: "s" }, d: 2 };
console.log(x.a.b, x.a.c, x.d);
x.a.b = 5;
console.log(x.a.b);
const inner: any = x.a;
console.log(inner.b);
inner.b = 9;
console.log(x.a.b);
// reflection on the nested object (anon-struct shape answered
// undefined here before)
const d: any = Object.getOwnPropertyDescriptor(x.a, "b");
console.log(d ? d.value : "absent", d ? d.writable : "-");
console.log(Object.keys(x.a).length);
// deep nesting
const y: any = { p: { q: { r: 3 } } };
console.log(y.p.q.r);
// nested object in mixed literal + for-in over inner
const m: any = { n: 1, o: { z: true }, s: "t" };
for (const k in m.o) {
  console.log("key", k, m.o[k]);
}
console.log(m.o.z);
// print shapes
console.log(x);
console.log(y.p);
