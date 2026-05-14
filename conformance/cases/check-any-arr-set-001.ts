// P0.10 — Array<Any>[i] = <concrete> indexed assignment with
// runtime box. Pre-fix typecheck rejected `Array<Any>[i] =
// Number` with strict-equality comparison ('type mismatch on
// element assignment: array of Any, value is Number'). Even when
// the typecheck was relaxed, the ssa-lower path used the regular
// 8-byte StoreDyn which corrupted the 16-byte Any-slot layout
// (tagged-slot stride). This commit ships the substrate piece:
//
// * runtime_str.c — `__torajs_arr_set_any(arr, i, tag, value)`
//   helper. Drops the old slot's heap value (if ANY_HEAP) before
//   overwriting with the new (tag, value) pair. Mirrors arr_push_
//   any but for indexed write.
//
// * ssa_lower.rs Index-assign path — when elem_ty is Any, pack
//   the RHS into a (tag, value) pair using the same scheme as
//   box_to_any but inlined (no heap alloc). Dispatch to the new
//   intrinsic with the four args. Tags: I64=2, F64=3 (bitcast),
//   Bool=1 (zext), Null=0, refcounted heap=4. Skips the generic
//   8-byte StoreDyn path entirely.
//
// * check.rs Index-assign typecheck — switch the strict ==
//   compare to is_assignable_to_resolved so Number → Any (and
//   String → Any, Bool → Any, etc.) all pass through the boxing
//   substrate above. ~20 cases unblocked across the broader
//   test262 sample.
//
// Out of scope at this commit: indexed write past the current
// length doesn't grow the array (sparse-write semantics) — JS
// does this implicitly but tora's substrate lands separately.
// Fixture exercises only the in-bounds overwrite path.

let xs: any[] = [10, 20, 30, 40]
console.log(xs.length)                       // 4

// Overwrite each slot with a different concrete type.
xs[0] = "swapped"
xs[1] = true
xs[2] = 99
xs[3] = null
console.log(xs[0])                           // swapped
console.log(xs[1])                           // true
console.log(xs[2])                           // 99
console.log(xs[3])                           // null

// Re-overwrite — heap-string slot's previous value gets dropped
// (refcount accounting balances).
xs[0] = "again"
xs[0] = 1
console.log(xs[0])                           // 1

// Mixed-type construction via slot writes.
let ys: any[] = [0, 0, 0]
ys[0] = "first"
ys[1] = 42
ys[2] = false
console.log(ys[0])                           // first
console.log(ys[1])                           // 42
console.log(ys[2])                           // false
