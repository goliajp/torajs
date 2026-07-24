// hof_arg_fns argc-ABI shadow guard: the receiving-fn gate resolves
// the callee by name only, so a callee name that is ALSO bound in
// any fn-local position is skipped (a local binding shadowing a
// same-named top-level fn used to pass the gate, argc-reshape the
// closure, and SIGBUS in the real receiver's normal-lane call).
// This baseline uses a unique receiver name — the mono-track argc
// injection must keep working for it.
function hofRecv(cb) {
  return cb(1, 2, 3);
}
console.log(hofRecv(function () { return arguments.length; }));
