// S317 — ES §20.1.2.6 Object.defineProperty(obj, key, desc, ...trailing)
// silently ignores args past index 2. tora's fixed sig
// `vec![Type::Any, Type::String, Type::Any]` rejected the 4th arg at
// typecheck; ssa_lower's `args.len() == 3` gate would also fall through
// to the generic dispatch panic.
//
// This fixture exercises:
// 1) trailing args are evaluated (side effects observable) — count == 3
//    means desc.value, trail1, trail2 all stepped exactly once.
// 2) evaluation order is left-to-right (desc before trail1 before trail2).
// 3) the descriptor is applied correctly (o.x === 42).

let n = 0
function step(label: string, v: any): any {
  n++
  console.log("step#" + n + ":" + label + "=" + v)
  return v
}

const o: any = {}
Object.defineProperty(
  o,
  "x",
  { value: step("desc", 42) },
  step("t1", "trail1"),
  step("t2", "trail2"),
)
console.log("o.x=", o.x)
console.log("count=", n)
