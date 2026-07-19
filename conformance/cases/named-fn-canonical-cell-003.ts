// RFC 20260717-namedfn-canonical-cell chunk 2 (naked-face leg) — the
// accessor-face mint for fns the forwarder collector doesn't rewrite
// (nested lifts) rides a per-fn hidden singleton slot, so two faces
// over the same nested fn compare equal across objects. (face ===
// bare-name for a NESTED fn stays a recorded O3 boundary: the
// collector runs before the nested lift, so its value read is not
// forwarder-canonicalized.)
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
}
outer();
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
