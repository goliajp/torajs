// RFC 20260809 B5 — AsyncDisposableStack (injected builtin):
// @@asyncDispose-first / @@dispose-fallback method resolution at
// use() time, null one-tick entry, adopt/defer awaited callbacks,
// disposeAsync idempotence (resolved promise), move, ReferenceError
// after disposal, SuppressedError aggregation across sync+async
// throws, and the [Symbol.asyncDispose] alias driven through
// `await using`.
async function main(): Promise<void> {
  const s1 = new AsyncDisposableStack();
  console.log(s1.disposed);
  const ra: any = { async [Symbol.asyncDispose]() { console.log("async a"); } };
  const rb: any = { [Symbol.dispose]() { console.log("sync b"); } };
  console.log("use-ret", s1.use(ra) === ra);
  s1.use(rb);
  console.log("use-null", s1.use(null));
  await s1.disposeAsync();
  console.log(s1.disposed);
  await s1.disposeAsync();

  const s2 = new AsyncDisposableStack();
  console.log("adopt-ret", s2.adopt(7, (v: any) => { console.log("adopt", v); }));
  console.log("defer-ret", s2.defer(() => { console.log("defer"); }));
  await s2.disposeAsync();

  const s3 = new AsyncDisposableStack();
  s3.use({ async [Symbol.asyncDispose]() { console.log("moved res"); } });
  const s4 = s3.move();
  console.log("s3.disposed", s3.disposed);
  console.log("s4.disposed", s4.disposed);
  await s3.disposeAsync();
  await s4.disposeAsync();

  try { s1.use(ra); } catch (e: any) { console.log("use-after:", e.name); }
  try { s1.move(); } catch (e: any) { console.log("move-after:", e.name); }

  const s5 = new AsyncDisposableStack();
  try { s5.use(123); } catch (e: any) { console.log("use-num:", e.name); }
  try { s5.adopt(1, 2); } catch (e: any) { console.log("adopt-nc:", e.name); }

  const s6 = new AsyncDisposableStack();
  s6.defer(() => { throw new Error("e1"); });
  s6.defer(async () => { throw new Error("e2"); });
  try { await s6.disposeAsync(); } catch (e: any) {
    console.log("agg:", e.name, e.error.message, e.suppressed.message);
  }

  {
    await using s7 = new AsyncDisposableStack();
    s7.defer(() => { console.log("via-await-using"); });
  }
  console.log("after-await-using");
}
main();
