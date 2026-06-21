// S320 — ES §28.1.6 Reflect.get(target, propertyKey, receiver?)
// silently ignores args past index 2 (receiver subset-deferred per
// the existing S257 typecheck-and-drop). ssa_lower's compile-time
// typed-struct + literal-key fast path widened the gate `== 2` to
// `>= 2` at S257 but never lowered args[2..], so step()-style
// trailing args were silent-dropped (count off by N vs bun, no
// runtime error to flag the bug).
//
// Probes:
// 1) trailing args evaluated — receiver + further trailing both
//    fire step()-counter increments.
// 2) evaluation order left-to-right (target → trailing — key is
//    a literal so order is fixed).
// 3) typed-struct field load result preserved (o.a === 1).
//
// Sister to S301 / S312 / S316 / S318 / S319 — same "comment-only
// or fast-path carve-out skipped lowering trailing" pattern.

let n = 0
function step(label: string, v: any): any {
  n++
  console.log("step#" + n + ":" + label + "=" + v)
  return v
}

const o = { a: 1, b: 2 }
// 3 trailing: receiver + 2 further trailing — all must fire step.
const r = Reflect.get(o, "a", step("t1", "trail1"), step("t2", "trail2"))
console.log("get=", r)
console.log("count=", n)
