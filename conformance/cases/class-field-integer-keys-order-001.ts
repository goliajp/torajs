// §10.1.11.1 OrdinaryOwnPropertyKeys: integer-index keys come first in
// ascending numeric order, then string keys in creation order — for a
// class instance's declared fields as much as for any object. Every
// enumeration surface reads the same order.
class C {
  b = "b";
  10 = "ten";
  a = "a";
  2 = "two";
  "07" = "not-an-index";
  m() { return 1; }
}
function mk(): any { return new C(); }
const c: any = mk();
console.log(Object.keys(c).join(","));
console.log(Object.getOwnPropertyNames(c).join(","));
const seen: string[] = [];
for (const k in c) seen.push(k);
console.log(seen.join(","));
console.log(JSON.stringify(c));
console.log(Object.entries(c).map(([k, v]) => k + "=" + v).join(","));
// an expando integer key sorts in with the declared ones
c[5] = "five"; c.z = "z";
console.log(Object.keys(c).join(","));
