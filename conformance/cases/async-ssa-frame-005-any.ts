// The three forms an `any` operand of a suspending await can take: a
// native promise, a user thenable, a plain value. Each resolves the way
// the await of its own type would.

async function awaitAny(x: any): Promise<any> {
  const v = await x;
  return v;
}

async function main(): Promise<void> {
  const p: any = Promise.resolve("from-promise");
  const t: any = { then(r: any) { r("from-thenable"); } };
  const plain: any = { k: 1 };
  console.log(await awaitAny(p));
  console.log(await awaitAny(t));
  const o = await awaitAny(plain);
  console.log(o.k, o === plain);
  console.log(await awaitAny(42), await awaitAny("str"), await awaitAny(null));
  const late: any = Promise.resolve(0).then(() => "late");
  console.log(await awaitAny(late));
}

main();
