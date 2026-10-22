// A binding annotated `Promise<number>` initialized from an `any` that
// holds a string settles the string; the handler's `v + 1` concatenates,
// and `await` on the derived promise reads the string it settled.

async function main() {
  const x: any = "s";
  const p: Promise<number> = Promise.resolve(x);
  const r = await p.then((v) => v + 1);
  console.log(typeof r, r);
  const y: any = 4;
  const q: Promise<number> = Promise.resolve(y);
  const s = await q.then((v) => v + 1);
  console.log(typeof s, s);
}
main();
