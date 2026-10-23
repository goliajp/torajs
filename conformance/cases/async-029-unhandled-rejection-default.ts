// P10.5-A3 — default unhandled-rejection reporter. Per spec
// §27.2.1.9 HostPromiseRejectionTracker, a rejected Promise that
// reaches the end of the microtask drain without ever being
// `.catch`'d / `.then(_, onErr)`'d / awaited fires the host's
// unhandled-rejection event. Bun's default behaviour is to write
// `error: <reason>\n` to stderr and exit with a non-zero status.
//
// Acceptance:
//   - stdout: "before reject\nafter reject call\n" (sync code keeps
//     running while the HPRT microtask is deferred to drain time).
//   - stderr: contains "error: boom" (bun adds extra trace lines —
//     conformance only diff-checks stdout, this comment documents
//     the stderr shape for manual review).
//   - exit code: non-zero (bun → 1, tr → 1 via main's
//     `__torajs_main_exit_code` read of UNHANDLED_REJECTION_OCCURRED).
//
// exit-code: 1

console.log("before reject")
Promise.reject("boom")
console.log("after reject call")
