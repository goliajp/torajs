// RFC 20260809 B6 — async generator [@@asyncDispose] (§27.1.6.1
// semantics carried on %AsyncGeneratorPrototype%; tr has no
// %AsyncIteratorPrototype% — recorded boundary): GetMethod(this,
// "return"), run it, answer a promise that settles undefined after
// the close completes — finally blocks run, and `await using` can
// hold an async generator. Observations are await-ordered: tr's
// async-gen return() resumes synchronously while bun enqueues (a
// pre-existing timing boundary), so the fixture only compares
// states both engines agree on at each await.
async function* ag() {
  try {
    yield 1;
    yield 2;
  } finally {
    console.log("cleanup");
  }
}
async function main(): Promise<void> {
  const a: any = ag();
  console.log(typeof a[Symbol.asyncDispose]);
  console.log((await a.next()).value);
  const p: any = a[Symbol.asyncDispose]();
  console.log(await p);
  console.log(typeof p.then);
  console.log((await a.next()).done);
  const fresh: any = ag();
  console.log(await fresh[Symbol.asyncDispose]());
  console.log((await fresh.next()).done);
}
async function useAg(): Promise<void> {
  await using u: any = ag();
  console.log((await u.next()).value);
}
async function run(): Promise<void> {
  await main();
  await useAg();
  console.log("after");
}
run();
