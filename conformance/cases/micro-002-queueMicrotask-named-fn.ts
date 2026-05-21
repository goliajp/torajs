// P10.1-A1.1 — queueMicrotask with a named-fn declaration cb. cb's
// SSA type is Type::FnSig (no env block), distinct from A1's
// closure-typed lambda cb path. ssa_lower dispatches between
// __torajs_queue_microtask_simple (this fixture) and
// __torajs_queue_microtask_closure (micro-001) based on cb type.

function mt() {
  console.log("mt-from-named-fn")
}

queueMicrotask(mt)
console.log("sync")
