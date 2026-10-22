// A typed handler on a promise of `any` settles a value of any kind:
// a value that does not fit its parameter runs it as written in JS, so
// the next handler reads what it returned, whatever the annotations say.
// The source is a promise of `any` directly, or a function declared
// `Promise<number>` that returns one.

function src(x: any): Promise<number> {
  return Promise.resolve(x);
}

const x: any = JSON.parse('"str"');
Promise.resolve(x)
  .then((v: number) => ({ a: v }), (e: any) => ({ a: 0 }))
  .then((w) => w.a)
  .then((r) => console.log("direct", typeof r, r));
src(x)
  .then((v: number) => ({ a: v }), (e: any) => ({ a: 0 }))
  .then((w) => w.a)
  .then((r) => console.log("declared", typeof r, r));
Promise.resolve(x)
  .then((v: number) => ({ a: v }))
  .then((w) => console.log("one leg", typeof w.a, w.a));
const n: any = 4;
src(n)
  .then((v: number) => ({ a: v + 1 }))
  .then((w) => console.log("number", typeof w.a, w.a));
