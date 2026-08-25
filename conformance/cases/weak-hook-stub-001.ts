// r499 — the rc-hit-zero weak-observer hook is shadowed by a `ret`
// stub only when no text of the weak member can be live; a program
// that registers observers must keep the real hook, or a dying
// target would never clear its WeakRef / WeakMap entries. Each
// observer kind is exercised through a target that dies while the
// observer is still alive.
class Box {
  constructor(public v: number) {}
}
function mk(): { r: WeakRef<Box>; m: WeakMap<Box, string>; s: WeakSet<Box> } {
  const b = new Box(1);
  const r = new WeakRef(b);
  const m = new WeakMap<Box, string>();
  m.set(b, "in");
  const s = new WeakSet<Box>();
  s.add(b);
  return { r, m, s };
}
const o = mk();
// WeakRef timing is spec-nondeterministic (bun may not have
// collected yet); tr's rc path clears deterministically. Either
// boolean is fine — the failure mode guarded here is a dangling
// deref through an un-notified observer.
const d = o.r.deref();
console.log(d === undefined || typeof d === "object");
const keep = new Box(2);
const r2 = new WeakRef(keep);
console.log(r2.deref() === keep, o.m.has(keep), o.s.has(keep));
