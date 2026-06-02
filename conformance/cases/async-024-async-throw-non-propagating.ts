// P10.5-A1 — async function body `throw` must NOT propagate
// synchronously to the caller. ES spec §27.7.3.6 AsyncFunctionBody:
// the body runs inside an implicit try/catch that converts any
// thrown value into a rejected Promise. Without this wrapping,
// `throw e` inside an `async fn` would short-circuit sync
// continuation at the call site — which was the pre-fix tr
// behavior (no stdout produced).
//
// Acceptance: the synchronous `console.log("after fail call")` MUST
// execute even though `fail()` body throws. The thrown value
// becomes a rejected Promise that is intentionally not awaited or
// .catch()'d in this minimal fixture (a wider unhandled-rejection
// handler hook lands in P10.5 sub-steps). Stdout-only bun-parity:
// stdout == "after fail call\n"; stderr divergence (bun's default
// "error: from-async" unhandled-rejection report) is outside the
// stdout-only diff conformance harness uses.

async function fail(): Promise<string> {
  throw "from-async";
}

fail();
console.log("after fail call");
