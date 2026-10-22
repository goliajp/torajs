// A `Promise<number>` parameter handed a promise settled with an `any`
// value: TS admits the call, and the body reads what was settled.

const x: any = JSON.parse('"str"');
function g(pp: Promise<number>) {
  return pp.then((v) => v + 1);
}
g(Promise.resolve(x)).then((v) => console.log("then", typeof v, v));
g(Promise.resolve(2)).then((v) => console.log("typed then", typeof v, v));
async function ag(pp: Promise<number>) {
  const v = await pp;
  console.log("await", typeof v, v);
}
ag(Promise.resolve(x));
ag(Promise.resolve(5));
