// A top-level `for await` over an async generator: each step awaits
// the promise `next()` answers (§14.7.5.6 step 6.d) before reading
// `done` and `value`. Stopping early by `break` closes the generator
// (§7.4.11 AsyncIteratorClose), so its `finally` runs once; running
// out runs it as the body ends.
const log: string[] = [];

async function* count(label: string, n: number) {
  try {
    for (let i = 1; i <= n; i++) {
      log.push(label + " yield " + i);
      yield i * 10;
    }
  } finally {
    log.push(label + " finally");
  }
}

for await (const v of count("a", 3)) log.push("a body " + v);

for await (const v of count("b", 5)) {
  log.push("b body " + v);
  if (v === 20) break;
}

log.push("done");
for (const line of log) console.log(line);
