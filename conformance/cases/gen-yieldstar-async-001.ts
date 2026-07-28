// async `yield* e` (F3, §27.5.3.2 generatorKind=async): delegation
// desugars to the for-await drive. A direct call to a known async
// generator skips the J.3 typed lane (its next() answers
// Promise<__step>) and rides the generic path; a bare-ident source
// and a sync iterable delegate the same way.
async function* inner() {
  yield 1;
  yield 2;
}
async function* outer_call() {
  yield 0;
  yield* inner();
  yield 3;
}
async function* outer_ident() {
  const g: any = inner();
  yield* g;
}
async function* outer_sync_src() {
  const a: any = [7, 8];
  yield* a;
  yield 9;
}
async function main() {
  const a: any = outer_call();
  for await (const v of a) console.log(v);
  const b: any = outer_ident();
  for await (const v of b) console.log("b", v);
  const c: any = outer_sync_src();
  for await (const v of c) console.log("c", v);
}
main();
