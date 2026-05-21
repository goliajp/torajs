// P10.1-A1.2 — nested queueMicrotask. Per WHATWG HTML §queueMicrotask
// a microtask cb is itself allowed to schedule further microtasks
// onto the queue; the drain processes them in the same drain cycle
// (runtime_promise.c::__torajs_microtask_run_until_idle line ~352
// loops while head < len, so tasks enqueued during a callback land
// at the tail and run before the loop exits).
//
// The substrate fix here is ast.rs::is_global_name + check.rs::
// is_known_builtin_global picking up "queueMicrotask" so the
// closure-capture analyzer no longer reports it as an unknown
// captured identifier when used from inside a cb body.

queueMicrotask(() => {
  console.log("mt-1")
  queueMicrotask(() => {
    console.log("mt-2")
    queueMicrotask(() => console.log("mt-3"))
  })
})
console.log("sync")
