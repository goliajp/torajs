// A top-level `new Promise<T>` whose executor resolves with an `any`
// value, read from a function body, settles that value as it is.

const x: any = "s";
const viaAny = new Promise<number>((r) => r(x));
let later = new Promise<number>((r) => r(x));

async function main() {
  const a = await viaAny;
  console.log("await any", typeof a, a);
  const b = await later;
  console.log("await let", typeof b, b);
}
main();
viaAny.then((v) => console.log("then any", typeof v, v));
