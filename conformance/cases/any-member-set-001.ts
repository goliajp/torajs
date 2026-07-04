// any-method-call RFC C4+ — tag-gated member writes on any
// receivers: dynobj set, RegExp lastIndex, Arr expando; primitives
// and unsupported cells raise catchable TypeErrors instead of
// corrupting the heap block.
const r: any = /ab/g;
r.lastIndex = 3;
console.log(r.lastIndex);
console.log(r.test("xxxab"));
console.log(r.lastIndex);
r.lastIndex = 0;
console.log(r.exec("ab cd ab")![0]);
console.log(r.lastIndex);
// non-global regex keeps the write too
const p: any = /cd/;
p.lastIndex = 7;
console.log(p.lastIndex);
// dynobj writes keep working, including growth + relocation
const o: any = { k0: 0 };
o.k1 = 1;
o.k2 = "two";
o.k3 = true;
o.k4 = 4.5;
o.k5 = 5;
o.k6 = 6;
o.k7 = 7;
o.k8 = 8;
o.k9 = 9;
console.log(o.k0, o.k2, o.k4, o.k9);
// any-typed payload through the same route
const v: any = "boxed";
o.k10 = v;
console.log(o.k10);
// primitive receivers reject loudly, not silently
try {
  const s: any = "hi";
  s.x = 1;
} catch (e) {
  console.log("caught-str");
}
try {
  const n: any = 5;
  n.x = 1;
} catch (e) {
  console.log("caught-num");
}
console.log("done");
