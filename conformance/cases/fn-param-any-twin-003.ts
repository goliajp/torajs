// An object-literal method with a fn-typed parameter reached through
// `any`, and a thenable whose `then` declares its resolve function's
// type: `Promise.resolve` reads `then` and calls it with the resolving
// functions (§27.2.2.2 NewPromiseResolveThenableJob).
function nf(v: number) {
  console.log("nf", v);
}
const o = {
  k(f: (v: number) => void) {
    f(9);
  },
};
const a: any = o;
a.k((v: number) => console.log("arrow", v));
a.k(nf);
o.k(nf);
const th = {
  then(res: (v: number) => void) {
    res(7);
  },
};
Promise.resolve(th).then((v) => console.log("thenable", v));
