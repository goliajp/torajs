// fn-name registry Phase 2 Step 6 — `const <name> = (...) => ...`
// arrow binding. ECMA-262 §10.2.10 NamedEvaluation sets
// `f.name = "f"` when the arrow flows through a binding initializer,
// so bun prints `[Function: f]`. tr's ssa_lower Pass 2 fn-decl walk
// only picks up named `function` declarations; arrow expressions
// land in the table via their (mangled) `__closure_*` synthesized
// name which Pass 2 filters out, so the runtime helper misses the
// fn_addr and emits `[Function (anonymous)]` instead. Tracked in
// L3b: `.name` property + bound-arrow NamedEvaluation for full
// spec parity. The `.expected` file documents the current tr form.
const f = (x: number) => x + 1;
console.log(f);
