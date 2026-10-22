// A top-level `Promise<number>` binding read from a function body: its
// handlers run, typed or settled with an `any` value.

const x: any = JSON.parse('"str"');
const p: Promise<number> = Promise.resolve(x);
const t: Promise<number> = Promise.resolve(3);

async function main() {
  const r = await p.then((v) => v + 1);
  console.log("any", typeof r, r);
  const s = await t.then((v) => v + 1);
  console.log("typed", typeof s, s);
}
function plain() {
  t.then((v) => console.log("plain", typeof v, v * 2));
}
main();
plain();
