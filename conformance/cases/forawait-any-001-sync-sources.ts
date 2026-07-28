// for-await over `any` sources riding the sync iteration protocol:
// a plain array binds elements verbatim, a Promise-element array
// awaits each element (§27.1.4.4 Async-from-Sync value await), and a
// sync generator's Promise-valued yield unwraps the same way.
const plain: any = [1, 2, 3];
const promises: any = [Promise.resolve(10), Promise.resolve(20)];
function* sg() {
  yield Promise.resolve(7);
  yield 8;
}
const gen: any = sg();
async function main() {
  for await (const v of plain) console.log(v);
  for await (const v of promises) console.log(v);
  for await (const v of gen) console.log(v);
}
main();
