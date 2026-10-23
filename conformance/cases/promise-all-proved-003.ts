// The open twin of promise-all-proved-001: an element of the any-shape
// input is a thenable, so the program spells `then` and the proof that
// lets Promise.all skip the `then` read does not hold. PromiseResolve
// has to read the element's `then` and adopt what it resolves with,
// and a `then` that throws rejects the result.
function withThenable(): any {
  return [Promise.resolve(1), { then(res: any) { res(2); } }, 3];
}
function withThrowingThen(): any {
  return [Promise.resolve(1), { then(_res: any) { throw "bad then"; } }];
}
async function main() {
  const a: Promise<number>[] = withThenable();
  const ra = await Promise.all(a);
  console.log(ra[0], ra[1], ra[2]);
  const b: Promise<number>[] = withThrowingThen();
  try {
    await Promise.all(b);
    console.log("b fulfilled");
  } catch (e) {
    console.log("b rejected", e);
  }
}
main();
