// Object-literal methods with fn-typed parameters: one reading its
// receiver, reached typed and through `any`; a function forwarding
// only its own fn-typed parameter to such a method; and a thenable
// whose `then` declares its resolve function's type.
function nf(v: number) {
  console.log("nf", v);
}
const o = {
  n: 5,
  k(f: (v: number) => void) {
    f(this.n);
  },
  pair(f: (v: number) => void, g: (v: number) => void) {
    f(1);
    g(2);
  },
};
o.k(nf);
o.k((v: number) => console.log("arrow", v));
o.pair(nf, (v: number) => console.log("second", v));
const a: any = o;
a.k(nf);
a.k((v: number) => console.log("recv", v));
a.pair(console.log, nf);
function onlyNamed(cb: (v: number) => void) {
  o.k(cb);
}
onlyNamed(nf);
const th = {
  then(res: (v: number) => void) {
    res(7);
  },
};
th.then((v: number) => console.log("direct", v));
Promise.resolve(th).then((v) => console.log("thenable", v));
