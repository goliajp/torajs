// Promise.all over arrays an `any` value reached, read four more ways:
// a destructuring of the result, a `.then` callback that annotates its
// parameter `number[]`, an array literal with an `any` element among
// promises, and a typed source joined with an escaped one in one alias
// class. Each element is what it settled with, whatever the annotation.
function mixed(): any {
  return [Promise.resolve(1), "two", { then(r: any) { r(3.5); } }];
}
function anyStr(): any { return "s"; }
function anyP(): any { return Promise.resolve(2); }
async function main() {
  const a: Promise<number>[] = mixed();
  const [x, y, z] = await Promise.all(a);
  console.log("destr", x, y, z, typeof y);
  const [, second] = await Promise.all(a);
  console.log("destr hole", second, typeof second);

  await Promise.all(a).then((v: number[]) => {
    console.log("then", v[0], v[1], v[2], v.length, typeof v[1]);
  });

  const lit: Promise<number>[] = [Promise.resolve(10), anyStr(), Promise.resolve(30)];
  const rl = await Promise.all(lit);
  console.log("literal", rl[0], rl[1], rl[2], typeof rl[1]);
  const inline = await Promise.all([Promise.resolve(1), anyStr() as Promise<number>]);
  console.log("inline literal", inline[0], inline[1], typeof inline[1]);
  const allAny: Promise<number>[] = [anyStr(), anyP()];
  const ra = await Promise.all(allAny);
  console.log("all-any literal", ra[0], ra[1], typeof ra[0]);
  const w = await Promise.race([anyStr(), Promise.resolve(1)]);
  console.log("race literal", w, typeof w);

  const typed: Promise<number>[] = [Promise.resolve(4), Promise.resolve(5)];
  let pick: Promise<number>[] = typed;
  const rt = await Promise.all(pick);
  console.log("typed first", rt[0] + rt[1]);
  pick = mixed();
  const re = await Promise.all(pick);
  console.log("escaped after", re[0], re[1], re[2], typeof re[1]);
  const both = [typed, mixed() as Promise<number>[]];
  for (const src of both) {
    const r = await Promise.all(src);
    console.log("joined", r.length, r[0], r[1], typeof r[1]);
  }
}
main();
