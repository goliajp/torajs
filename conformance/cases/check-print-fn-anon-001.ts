// fn-name registry Phase 2 Step 6 — purely anonymous arrow expressed
// inline at the console.log call site. Neither `function <name>` nor
// a binding initializer, so no fn-name registry row exists. Chunk
// 797 aligned the registry-miss print with bun's `[Function]`
// spelling; the .expected pin of tr's old `[Function (anonymous)]`
// divergence is retired — the live bun oracle applies again.
console.log(() => {});
