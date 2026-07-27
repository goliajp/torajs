// `await <any>` by-VALUE dispatch (__torajs_anyv_await): a heap
// Promise cell in an `any` unwraps to its settled value boxed per the
// cell's repr stamp; every other runtime form passes through identity.
// Covers: promise-in-any (number / string / struct settled), plain
// any (number / string / null / undefined / struct), rejection
// propagation into try/catch, and the for-await element lane over
// Array<any> with a promise mixed in (used to bind verbatim as
// `Promise { <resolved> }`).
async function main() {
  const a: any = Promise.resolve(41);
  console.log("v", await a);
  const b: any = 7;
  console.log("w", await b);
  const s: any = "hello";
  console.log("s", await s);
  const n: any = null;
  console.log("n", await n);
  const u: any = undefined;
  console.log("u", await u);
  const o: any = {k: 3};
  const oo = await o;
  console.log("o", oo.k);
  const ps: any = Promise.resolve("boxed str");
  console.log("ps", await ps);
  const ph: any = Promise.resolve({deep: true});
  const phv = await ph;
  console.log("ph", phv.deep);
  try {
    const r: any = Promise.reject("nope");
    await r;
    console.log("unreachable");
  } catch (e) {
    console.log("caught", e);
  }
  const xs: any[] = [1, Promise.resolve(2), "s", null];
  for await (const x of xs) {
    console.log("x", x);
  }
}
main();
