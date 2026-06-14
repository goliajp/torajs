// fn-name registry Phase 2 Step 6 — purely anonymous arrow expressed
// inline at the console.log call site. Neither `function <name>` nor
// a binding initializer; bun lands `[Function]` with no name, while
// tr's runtime helper returns `[Function (anonymous)]` for any
// fn_addr that doesn't appear in `__torajs_fn_name_table[]`. The
// .expected file documents the current tr form; the gap with bun
// (no parens, no "anonymous" word) is a printer-shape question
// tracked in L3b alongside `.name` spec parity.
console.log(() => {});
