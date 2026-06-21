// S321 — ES §22.1.3.{18,19} `s.replace(re, repl, ...trailing)` and
// `s.replaceAll(re, repl, ...trailing)` silently ignore args past
// (regex, replacement). Pre-S321 the regex-receiver dispatch arm in
// ssa_lower's `s.<method>(re, ...)` block had a
// `debug_assert_eq!(args.len(), 2)` carve-out: in release builds
// the assert is disabled, so args[2..] were silent-dropped with no
// side-effect lowering. Sister to S286 (match / matchAll) on the
// same dispatch family.
//
// Probes:
// 1) trailing args evaluated — single + double trailing fire step
//    counter for both `replace` and `replaceAll`.
// 2) evaluation order left-to-right (replacement before trailing).
// 3) result correctness preserved (replaceAll replaces every match,
//    replace replaces only the first).

let n = 0
function step(label: string, v: any): any {
  n++
  console.log("step#" + n + ":" + label + "=" + v)
  return v
}

const s = "a-b-c-d"

const r1 = s.replace(/-/, "X", step("t1", "trail1"))
console.log("replace=", r1)

const r2 = s.replaceAll(/-/g, "Y", step("t2", "trail2"), step("t3", "trail3"))
console.log("replaceAll=", r2)

console.log("count=", n)
