// rotation 552 (551-06) — `for await (const v of ag())` over an async
// generator CALL used to take the parse-time next-loop desugar (the sync
// generator fix removed it); it now rides the iterator-protocol lane's
// async half like an Ident source does. Kept apart from the sync fixture:
// a sync generator with `try/finally` in the same program makes the async
// step's `value` read back as f64 bits (552-04, pre-existing, independent
// of the lane).
async function* ag(): AsyncGenerator<number> {
  yield 10;
  yield 20;
  yield 30;
}
(async () => {
  let t = 0;
  for await (const v of ag()) {
    t += v;
    if (v === 10) continue;
    if (v === 20) break;
  }
  const held = ag();
  let u = 0;
  for await (const v of held) {
    u += v;
  }
  console.log("await", t, u);
})();
