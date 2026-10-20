// A `Promise<number>` a function returns from `any` keeps its fraction
// at every reader (ECMA-262 §27.2.5.4, §27.7.5.3): an async function
// returning an `any` value, a plain function returning a promise held
// in an `any[]`, and the same two with another path that rejects with
// an `Error` (`Promise.reject`, `throw`), read by `await` and by a
// handler declaring a `number` parameter. Each fraction comes from
// where the program's types cannot see it (JSON text, an `any[]`).

function frac(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
const box: any[] = [frac(5), frac(9)];

async function fromAny(): Promise<number> {
  const h: any = JSON.parse("6.5");
  return h;
}
function fromBox(): Promise<number> {
  const h: any = box[0];
  return h;
}
function rejectOrBox(b: boolean): Promise<number> {
  if (b) return Promise.reject(new Error("rejected"));
  const h: any = box[1];
  return h;
}
async function throwOrAny(b: boolean): Promise<number> {
  if (b) throw new Error("thrown");
  const h: any = JSON.parse("8.5");
  return h;
}

async function main() {
  const v = (await fromAny()) + 1;
  console.log("async any", v);
  const w = (await fromBox()) + 1;
  console.log("plain box", w);
  const x = (await rejectOrBox(false)) + 1;
  console.log("reject or box", x);
  try {
    await rejectOrBox(true);
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
  const y = (await throwOrAny(false)) + 1;
  console.log("throw or any", y);
  try {
    await throwOrAny(true);
  } catch (e) {
    console.log("caught", (e as Error).message);
  }
  await fromAny().then((n: number) => console.log("then async any", n * 2));
  await fromBox().then((n: number) => console.log("then plain box", n * 2));
  await rejectOrBox(false).then((n: number) => console.log("then reject or box", n * 2));
  await throwOrAny(false).then((n: number) => console.log("then throw or any", n * 2));
}
main();
