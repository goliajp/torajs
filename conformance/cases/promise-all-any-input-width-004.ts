// A `Promise<T>[]` literal that spreads an `any` value stores what the
// spread hands over as is (§13.2.4.1, §13.2.4.2): a string beside the
// promises settles as the string under Promise.all, Promise.race and an
// await of the element.
function mixed(): any { return ["s", Promise.resolve(2)]; }
function typed(): Promise<number>[] { return [Promise.resolve(5)]; }
const top: Promise<number>[] = [Promise.resolve(1), ...mixed()];
const tr = await Promise.all(top);
console.log("top all", tr, tr[1], tr[2] + 1);
async function main() {
  const lit: Promise<number>[] = [Promise.resolve(1), ...mixed()];
  const r = await Promise.all(lit);
  console.log("all", r, r[1], r[2] + 1);
  const only: Promise<number>[] = [...mixed()];
  console.log("race", await Promise.race(only));
  console.log("elem", await only[0], (await only[1]) + 1);
  const t: Promise<number>[] = [Promise.resolve(3), ...typed()];
  const s = await Promise.all(t);
  console.log("typed spread", s[0] + s[1]);
}
await main();
