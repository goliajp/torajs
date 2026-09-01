// 552-04 — an async generator's `{value, done}` step is reached through
// `Promise<__step_*>`, which is type-erased, so the width injection
// every other annotation-consuming site takes was skipped on the READ
// side while the literal that builds the step took it. A `try`/`finally`
// generator anywhere in the program floats the shared step class, and
// then an integer yield read back as its own f64 bit pattern.
const log: string[] = [];
function* sync(tag: string): Generator<number> {
  try {
    yield 1;
  } finally {
    log.push("c" + tag);
  }
}

async function* ag(): AsyncGenerator<number> {
  yield 10;
  yield 20;
}

(async () => {
  // `await it.next()` + member read — the Promise-payload lane.
  const it = ag();
  const r = await it.next();
  console.log(JSON.stringify(r), r.value + 1);

  // `for await` — the iterator-protocol lane, a separate recovery.
  for await (const v of ag()) console.log(v);

  // the sync lane, which was always right, still is
  for (const v of sync("x")) console.log(v);
  console.log(log.join(","));
})();
