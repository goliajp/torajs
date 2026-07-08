// fn-name registry — `const <name> = (...) => ...` arrow binding.
// ECMA-262 §10.2.10 NamedEvaluation sets `f.name = "f"` when the
// arrow flows through a binding initializer, so both engines print
// `[Function: f]`. Chunk 720 closed the gap: Pass 2B recovers the
// binding name for lifted `__closure_*` bodies and pushes a fn-name
// registry row, so the runtime helper resolves the fn_addr. The
// former `.expected` pin (documenting the pre-720 anonymous form)
// is retired — this now runs against the bun oracle directly.
const f = (x: number) => x + 1;
console.log(f);
