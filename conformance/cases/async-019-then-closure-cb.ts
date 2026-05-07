// T-15.g.5 (v0.5.0) — Promise<T>.then(cb) where cb is a CAPTURING
// closure (env-pointer value, not raw fn pointer). The runtime
// dispatcher must load fn_addr from env+CLOSURE_FN_ADDR_OFF and
// pass `(env, value)` rather than just `(value)`. Selection
// between the two dispatchers happens at the .then call site
// based on cb's static type (Type::Closure vs Type::FnSig).
//
// Two substrate fixes paired in this fixture:
//   1. synthesize_main now primes escape_captured_lets via
//      collect_closure_captures_in_stmt before lowering top-level
//      let-decls; previously top-level `let x = 10` always alloca'd
//      stack, and the closure construction stored that stack ptr
//      into env+CAP — env_drop would then call obj_drop(stack_ptr)
//      → SIGABRT during shutdown.
//   2. ssa_lower's `.then` dispatch picks
//      __torajs_promise_then_closure (new C helper) when cb's
//      static type is Type::Closure. Without it the simple
//      dispatcher treated env_ptr as fn_ptr and jumped into
//      env+0 (heap header) → SEGV mid-microtask.

let x = 10
let cb = function(v: number): number { return v + x }

let p = Promise.resolve(5).then(cb)
console.log(await p)                      // 15

// Multi-closure shared-capture exercises the capture-box ARC: both
// closures inc the box's refcount at construction; each env_drop
// dec's; box frees at zero. Without ARC two env_drops would each
// free the same box → double free at shutdown.
let y = 100
let cb2 = function(v: number): number { return v + x + y }
let p2 = Promise.resolve(2).then(cb2)
console.log(await p2)                     // 112
