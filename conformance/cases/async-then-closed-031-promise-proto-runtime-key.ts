// `Promise.prototype[k] = f` with `k` taken at run time. An await of a
// native promise never reads the replaced method (§27.7.5.3 Await runs
// PerformPromiseThen directly); an explicit `.then` call does.

const k: string = process.argv[2] ?? ["th", "en"].join("");
const orig = (Promise.prototype as any)[k];
let seen = 0;
(Promise.prototype as any)[k] = function (this: any, a: any, b: any) {
  seen = seen + 1;
  return orig.call(this, a, b);
};

async function main() {
  const xs: number[] = [];
  for (let i = 0; i < 3; i++) {
    xs[i] = await Promise.resolve(i * 2);
  }
  console.log(xs.join(","), seen);
  const y = await Promise.resolve(5).then((v: number) => v + 1);
  console.log(y, seen);
}

main();
