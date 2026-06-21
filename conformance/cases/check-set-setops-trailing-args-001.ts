// S318 — ES §24.2.5.{4-10} Set ES2025 setops trailing-arg ignore.
// spec sigs are 1-arg (other SetLike); trailing args MUST be silently
// ignored. tora's fixed `vec![Type::Set]` rejected at typecheck with
// "expected 1 argument(s), got M"; ssa_lower's
// `debug_assert_eq!(args.len(), 1)` carve-out (release build dropped
// assert) would also silent-drop trailing args' side effects.
//
// This fixture exercises:
// 1) trailing args evaluated (step counter ticks) — for both read-only
//    setops (isSubsetOf/isDisjointFrom) and mutating ones (union/
//    intersection).
// 2) evaluation order is left-to-right (`other` Set before trailing).
// 3) return values are correct (subset bool / set size).
//
// Each call extracted to its own stmt before console.log so the
// step()-counter probes interleave deterministically with the result
// print (avoids pre-existing console.log multi-arg ordering quirk in
// which inline call args print "header" before the inner step()s).

let n = 0
function step(label: string, v: any): any {
  n++
  console.log("step#" + n + ":" + label + "=" + v)
  return v
}

const a = new Set([1, 2, 3])
const b = new Set([1, 2, 3, 4])

const subset = a.isSubsetOf(b, step("t1", "trail1"), step("t2", "trail2"))
console.log("subset=", subset)

const disjoint = a.isDisjointFrom(b, step("t3", "trail3"))
console.log("disjoint=", disjoint)

const u = a.union(b, step("t4", "trail4"))
console.log("union-size=", u.size)

const ix = a.intersection(b, step("t5", "trail5"))
console.log("inter-size=", ix.size)

console.log("count=", n)
