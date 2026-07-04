// any-method-call RFC C4+ — tag-gated member reads on any
// receivers: arr expando round-trip, definite undefined on
// non-object receivers, TypeError on null/undefined.
const a: any = [1, 2];
a.foo = "bar";
console.log(a.foo);
console.log(a.nothere);
console.log(a.length);
// dynobj reads keep working
const o: any = { k: 42 };
console.log(o.k);
console.log(o.missing);
// arbitrary props on non-object cells answer undefined
const s: any = "hi";
console.log(s.foo);
const m: any = new Map();
console.log(m.foo);
const d: any = new Date(0);
console.log(d.foo);
// primitives too
const n: any = 5;
console.log(n.foo);
// null / undefined receivers throw catchably
try {
  const z: any = null;
  console.log(z.foo);
} catch (e) {
  console.log("caught-null");
}
try {
  const u: any = undefined;
  console.log(u.foo);
} catch (e) {
  console.log("caught-undef");
}
console.log("done");
