// chunk 620 — a WeakRef whose target dies via the CYCLE COLLECTOR
// (not the normal drop path) must read as cleared: collect_white now
// fires weakref_target_dying like obj_drop_rc does. WeakRef timing
// is spec-nondeterministic, so the assertion accepts both cleared
// and still-alive (bun may not have collected) — the pre-fix tr
// failure mode was a dangling deref, not a wrong boolean.
class N2 {
  other: any = null;
}
let wr = new WeakRef(new N2());
{
  const a = new N2();
  const b = new N2();
  a.other = b;
  b.other = a;
  wr = new WeakRef(a);
}
for (let i = 0; i < 2000; i++) {
  const a = new N2();
  const b = new N2();
  a.other = b;
  b.other = a;
}
const d = wr.deref();
console.log(d === undefined || typeof d === "object");
console.log("done");
