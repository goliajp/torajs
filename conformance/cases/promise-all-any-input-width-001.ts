// Promise.all over an array that reached a `Promise<number>[]` slot from
// an `any` value. The annotation has no runtime meaning: the elements
// are whatever the any side put there (a promise, a thenable, a plain
// number), and §27.2.4.1.2 settles each through PromiseResolve. The
// result array therefore holds values of any kind, and it is read on the
// any lane rather than at the element width the annotation names.
function withThenable(): any {
  return [Promise.resolve(1), { then(res: any) { res(2); } }, 3];
}
function withThrowingThen(): any {
  return [Promise.resolve(1), { then(_res: any) { throw "bad then"; } }];
}
function withString(): any {
  return [Promise.resolve(1), { then(res: any) { res("two"); } }, 3.5];
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
  const c: Promise<number>[] = withString();
  const rc = await Promise.all(c);
  console.log(rc.length, rc[0], rc[1], rc[2], typeof rc[1]);
  const inline = await Promise.all(withString() as Promise<number>[]);
  console.log(inline[0], inline[1], inline[2]);
  Promise.all(c).then((v) => console.log("then", v[0], v[1], v.length));
}
main();
