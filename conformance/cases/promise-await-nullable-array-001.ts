// An awaited promise of a nullable array of nullable elements reads both
// levels as themselves, and every producer of a `Promise<(T | null)[]>`
// hands back the array it was given.

async function mk(): Promise<(number | undefined)[] | undefined> {
  return [1, undefined, 3];
}
async function none(): Promise<(number | undefined)[] | undefined> {
  return undefined;
}
async function strs(): Promise<(string | null)[] | null> {
  return ["p", null];
}
async function unann() {
  return ["a", null];
}

async function main(): Promise<void> {
  const a = await mk();
  if (a !== undefined) console.log("nested", a, a[1], a[0] + 1);
  const b = await none();
  console.log("absent", b, b === undefined);
  const s = await strs();
  if (s !== null) console.log("strings", s, s[1] === null, s[0]);
  const c = await unann();
  console.log("unannotated", c, c[1] === null);
  const xs: (string | null)[] = ["x", null];
  const d = await Promise.resolve(xs);
  console.log("resolve", d, d[1] === null, d[0]);
  const e = await new Promise<(string | null)[]>((r) => r(xs));
  console.log("executor", e, e[1] === null, e[0]);
  const ys: (number | null)[] = [1, null];
  const f = await Promise.resolve(ys);
  console.log("numbers", f, f[1] === null, f[0] + 1);
}
main();
