// RC-4 F6 (RFC 20260706-test262-bug-corpus / RFC 20260705 Phase
// 1.5) — an inline closure literal passed as a call argument drops
// its env right after the call. Under the Phase 1 hand-the-stake
// protocol the env inherited the outer binding's only reference, so
// env-drop freed the captured cell while the outer name stayed live
// — every later use was a UAF (surfaced as WeakMap buckets=null
// SIGSEGV under throw-unwind timing, latent for Map/heap-churn).
// Shared-capture accounting gives every env its own +1 stake; the
// outer binding keeps its own and scope close releases it.

function callIt(thunk: () => void): void {
  try {
    thunk();
  } catch (e: number) {}
}

// WeakMap: reject-throw inside the closure (pending-throw unwind),
// then keep using the captured receiver at top level.
let s = new WeakMap();
callIt(function (): void {
  s.set(1, 1);
});
let k1 = {};
s.set(k1, 7);
console.log(s.has(k1));

// Map + heap churn after the closure call — the latent form that
// crashed once the freed cell's memory was reused.
let m = new Map();
callIt(function (): void {
  m.set(1, 3);
});
let junk: any[] = [];
for (let i = 0; i < 2000; i++) {
  junk.push({ a: i, b: "xxxxxxxxxxxxxxxx" });
}
m.set(2, 7);
console.log(m.get(1), m.get(2), m.size);

// Str capture: outer binding stays usable after the inline-arg
// closure is gone.
let name = "torajs-" + m.size;
callIt(function (): void {
  let n = name.length;
});
console.log(name);
