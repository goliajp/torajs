// S301 — WeakMap.{set,get,has,delete} + WeakSet.{add,has,delete}(useful,
// ...trailing) trailing-arg ignore per ES §24.3.3.* / §24.4.3.*. Spec
// reserves slots past useful args; tora's helpers are 2- (WeakMap.set)
// or 1-arg only. Trailing operands must be evaluated for side effects
// then dropped — step()-counter probe falsified prior coverage:
// `expected 1 argument(s), got 2` strict reject before this fix.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

class K {}
const k1 = new K();
const k2 = new K();
const k3 = new K();

const wm = new WeakMap<K, number>();
const ws = new WeakSet<K>();

// WeakMap.set(k, v, ...trailing)
calls = 0;
wm.set(k1, 10, step("wm-set1") as any, step("wm-set2") as any);
console.log("wm.set calls=", calls, "has=", wm.has(k1));

// WeakMap.get(k, ...trailing) — value-return is pre-existing bug
// (returns undefined regardless; L3b). Probe only the side-effect counter.
calls = 0;
wm.get(k1, step("wm-get1") as any);
console.log("wm.get calls=", calls);

// WeakMap.has(k, ...trailing)
calls = 0;
const has1 = wm.has(k1, step("wm-has1") as any, step("wm-has2") as any);
console.log("wm.has calls=", calls, "v=", has1);

// WeakMap.delete(k, ...trailing)
calls = 0;
const del1 = wm.delete(k1, step("wm-del1") as any);
console.log("wm.delete calls=", calls, "v=", del1, "has=", wm.has(k1));

// WeakSet.add(k, ...trailing)
calls = 0;
ws.add(k2, step("ws-add1") as any, step("ws-add2") as any);
console.log("ws.add calls=", calls, "has=", ws.has(k2));

// WeakSet.has(k, ...trailing)
calls = 0;
const has2 = ws.has(k2, step("ws-has1") as any);
console.log("ws.has calls=", calls, "v=", has2);

// WeakSet.delete(k, ...trailing)
calls = 0;
const del2 = ws.delete(k3, step("ws-del1") as any, step("ws-del2") as any);
console.log("ws.delete calls=", calls, "v=", del2);
