// P10.5-A2 — async-fn body `throw <expr>` where the thrown value's
// type does not match the declared inner return T. ES spec
// §27.7.3.6 AsyncFunctionBody runs the body inside an implicit
// `try { ... } catch (__async_err: any) { return Promise.reject(
// __async_err); }`. The catch param is `any` (not the inner T),
// so the user can `throw` a value whose static type differs from
// the fn's declared return inner T.
//
// Pre-P10.5-A2 the catch_type was pinned to `inner_ty` as a
// narrow MVP workaround because Promise.reject only accepted T
// from a fixed primitive/heap whitelist (string-string overlap is
// what made async-024 pass under the narrow shape). With A2 the
// catch_type is `any` and Promise.reject accepts Type::Any,
// dispatching to the existing heap-reject path (boxed-any
// pointer fits an i64 reason slot; drop walks the universal
// heap header).
//
// Acceptance: typecheck passes, `console.log("after fail call")`
// runs even though fail() rejects with a string while declaring
// `Promise<number>`. Stdout-only bun-parity:
//   stdout == "after fail call\n"
// stderr divergence (bun's default unhandled-rejection report)
// is outside the stdout-only diff the conformance harness uses.

async function fail(): Promise<number> {
  throw "from-async"
}

fail()
console.log("after fail call")
