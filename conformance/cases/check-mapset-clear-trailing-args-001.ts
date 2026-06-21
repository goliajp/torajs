// S300 — {Set,Map}.clear(...trailing) trailing-arg ignore per ES
// §24.2.3.5 / §23.1.3.3. Spec is 0-arg; trailing operands must be
// evaluated for side effects then dropped. check.rs S264 already
// typecheck-and-drop; ssa_lower mirror was missing the lower-and-drop
// loop — silent-drop probed via step()-counter.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

const m = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
const s = new Set<number>([1, 2, 3, 4]);

// Map.clear with 2 trailing
calls = 0;
m.clear(step("mc1") as any, step("mc2") as any);
console.log("map.clear calls=", calls, "size=", m.size);

// Set.clear with 2 trailing
calls = 0;
s.clear(step("sc1") as any, step("sc2") as any);
console.log("set.clear calls=", calls, "size=", s.size);

// Re-populate then 1 trailing
m.set("x", 1);
s.add(99);
calls = 0;
m.clear(step("mc-one") as any);
console.log("map.clear-1 calls=", calls, "size=", m.size);
calls = 0;
s.clear(step("sc-one") as any);
console.log("set.clear-1 calls=", calls, "size=", s.size);

// 3 trailing
m.set("y", 2);
s.add(7);
calls = 0;
m.clear(step("m3a") as any, step("m3b") as any, step("m3c") as any);
console.log("map.clear-3 calls=", calls, "size=", m.size);
calls = 0;
s.clear(step("s3a") as any, step("s3b") as any, step("s3c") as any);
console.log("set.clear-3 calls=", calls, "size=", s.size);
