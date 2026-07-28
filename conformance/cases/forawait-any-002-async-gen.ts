// for-await over an async generator object erased to `any` — the
// derived iterator's next() answers Promise<IteratorResult>, awaited
// per §14.7.5.6 step 6.d before done/value are read; a `break` exits
// through the loop's teardown without disturbing later output.
async function* ag() {
  yield 1;
  yield 2;
  yield 3;
}
const g1: any = ag();
const g2: any = ag();
async function main() {
  for await (const v of g1) console.log(v);
  for await (const v of g2) {
    console.log("b", v);
    if (v === 2) break;
  }
  console.log("done");
}
main();
