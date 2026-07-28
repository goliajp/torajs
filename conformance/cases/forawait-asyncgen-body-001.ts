// for-await inside an async generator body (F3): the F1 manual
// protocol carries the await drive — an @@asyncIterator-less sync
// source rides the sync symbol with per-value await, a Promise
// element unwraps, and an inner async generator's Promise-shaped
// step settles before done/value are read.
async function* over_sync() {
  const src: any = [1, 2, 3];
  for await (const v of src) {
    yield v * 10;
  }
}
async function* inner_ag() {
  yield 100;
  yield 200;
}
async function* over_async() {
  const g: any = inner_ag();
  for await (const v of g) {
    yield v + 1;
  }
}
async function* over_promises() {
  const src: any = [Promise.resolve(7), Promise.resolve(8)];
  for await (const v of src) {
    yield v;
  }
}
async function main() {
  const a: any = over_sync();
  for await (const v of a) console.log(v);
  const b: any = over_async();
  for await (const v of b) console.log(v);
  const c: any = over_promises();
  for await (const v of c) console.log(v);
}
main();
