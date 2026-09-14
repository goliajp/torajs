// RFC 20260915 blade 4 — a `for await` with no yield in its body inside
// an async GENERATOR: it used to fall through the machine's catch-all
// onto the synchronous lowering (the whole generator ran inside the
// first `next()`); now every `for await` in a machine takes the manual
// protocol with a marked suspend per step, so "sync" lands first and
// the consumer's `for await` over the generator interleaves with it.
const log: string[] = [];
async function* gen(): AsyncGenerator<number> {
  for await (const x of [1, 2, 3]) { log.push("g" + x); if (x === 2) break; }
  yield 7;
  for (const y of [4, 5]) { await null; log.push("y" + y); yield y; }
}
async function f(): Promise<void> {
  for await (const v of gen()) { log.push("v" + v); }
}
const p = f(); log.push("sync");
p.then(() => console.log("agen", log.join(",")));
