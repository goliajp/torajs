// P10.5-A4 — `process.on('unhandledRejection', cb)` suppresses the
// default `error: <reason>` reporter and exit-1 signal. ES spec
// doesn't standardise this (it's Node/Bun's `EventEmitter#on`
// extension on the `process` object); torajs adopts the bun-
// compatible shape so unhandled-rejection observability is
// programmable.
//
// Narrow v0.5 MVP: only `'unhandledRejection'` is wired; cb is
// invoked with `reason` boxed as an `AnyValue` (`reason_any`
// argument). `process.on('rejectionHandled', cb)` is unreachable
// under the sync MVP and tracked as L3b follow-up.
//
// Acceptance:
//   - stdout: "before reject\nafter reject call\nhandler fired\n"
//   - stderr: empty (no `error:` default reporter)
//   - exit code: 0 (UNHANDLED_REJECTION_OCCURRED stays 0 because
//     `unhandled::sweep_unhandled_list` dispatches the listener
//     and never sets the flag).
// Conformance harness compares stdout strictly; bun's exit 0 is
// also what we match here (no `skip` annotation needed).

function onReject(reason_any: any): void {
  console.log("handler fired")
}

process.on('unhandledRejection', onReject)

console.log("before reject")
Promise.reject("boom")
console.log("after reject call")
