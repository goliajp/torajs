// RFC 20260717-namedfn-canonical-cell chunk 2 (naked-face leg) + O3
// pass-reorder: the nested-fn lift now runs before the fn-to-closure
// collector, so a nested fn's VALUE reads join the forwarder
// canonical-cell lane and its accessor faces ride the per-fn hidden
// singleton slot — face === face AND face === bare-name both hold,
// and expando writes land on the one fn object.
function outer() {
  function inner() { return 5; }
  const o: any = {};
  Object.defineProperty(o, "x", { get: inner });
  const p: any = {};
  Object.defineProperty(p, "y", { get: inner });
  const d: any = Object.getOwnPropertyDescriptor(o, "x");
  const e: any = Object.getOwnPropertyDescriptor(p, "y");
  console.log(d.get === e.get, o.x, p.y);
  const q: any = {};
  Object.defineProperty(q, "z", { get: inner });
  console.log(Object.getOwnPropertyDescriptor(q, "z").get === d.get);
  console.log(d.get === inner);
  const a: any = inner;
  const b: any = inner;
  console.log(a === b, a === inner);
  a.tag = "marked";
  console.log(b.tag, typeof a, a());
}
outer();
function outer5() {
  function inner5(n: number): number { return n * 2; }
  console.log(inner5(3));
  const arr = [1, 2, 3];
  console.log(arr.map(inner5).join(","));
}
outer5();
function top() { return 42; }
const t: any = {};
Object.defineProperty(t, "v", { get: top });
const u: any = {};
Object.defineProperty(u, "w", { get: top });
console.log(
  Object.getOwnPropertyDescriptor(t, "v").get === Object.getOwnPropertyDescriptor(u, "w").get,
  Object.getOwnPropertyDescriptor(t, "v").get === top,
  t.v + u.w,
);
