// S319 — ES §23.1.3.11 Array.prototype.flatMap(callback, thisArg)
// silently ignores args past thisArg per ES trailing-arg rule.
// tora callbacks lack `this` binding so thisArg itself is also
// dropped (S270 typecheck-drop covers the check.rs side for the full
// Array callback family; this fixture exercises the ssa_lower mirror
// for flatMap).
//
// Pre-S319 ssa_lower's `args.len() == 1` gate rejected `args.len() >=
// 2` shape with "ssa-lower: unsupported member call shape: flatMap"
// (check.rs typecheck-drops args[1..] for the callback family but the
// SSA dispatch arm read only args[0] and panicked on the unhandled
// shape).
//
// Probes:
// 1) trailing args evaluated (step counter) — undefined thisArg + 2
//    trailing args.
// 2) evaluation order left-to-right (callback before trailing args).
// 3) result correctness (flatMap doubles each element).

let n = 0
function step(label: string, v: any): any {
  n++
  console.log("step#" + n + ":" + label + "=" + v)
  return v
}

const a = [1, 2, 3]
const r = a.flatMap((x: number) => [x * 2], step("t1", "trail1"), step("t2", "trail2"))
console.log("fm-len=", r.length)
console.log("fm-0=", r[0])
console.log("fm-1=", r[1])
console.log("fm-2=", r[2])
console.log("count=", n)
