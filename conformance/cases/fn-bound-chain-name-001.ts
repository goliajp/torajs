// a chain of bound functions: each layer holds only its target, this
// and partial arguments; the name spells one `bound ` per layer

// typed targets
const add = (a: number, b: number): number => a + b;
const add5 = add.bind(null, 5);
console.log(add5(3), JSON.stringify(add5.name), add5.length);

function join3(a: string, b: string, c: string): string {
  return a + b + c;
}
const j1 = join3.bind(null, "x");
const j2 = j1.bind(null, "y");
console.log(j2("z"), j2.name, j2.length, j1.name, j1.length);

// any-typed chain, partial arguments on each layer
let h: any = function digits(a: number, b: number, c: number) {
  return a * 100 + b * 10 + c;
};
h = h.bind(null, 1);
h = h.bind(null, 2);
console.log(h(3), h.name, h.length);

// a long any-typed chain with no partial arguments
const N = 50000;
let f: any = function base(this: any, x: number) {
  return x + 1;
};
for (let i = 0; i < N; i++) f = f.bind(null);
const nm: string = f.name;
console.log(typeof f, f.length, nm.length, nm.slice(0, 13), nm.slice(-11));
console.log(nm === "bound ".repeat(N) + "base", f(41));

// a short chain is callable through every layer
let c: any = function inc(x: number) {
  return x + 1;
};
for (let i = 0; i < 200; i++) c = c.bind(null);
console.log(c(41), c.name.length);

// partials from every layer, innermost first
let s: any = function (...xs: number[]) {
  return xs.join(",");
};
for (let i = 0; i < 5; i++) s = s.bind(null, i);
console.log(s(9), s(), s.length);

// the innermost bound this wins
function getV(this: any) {
  return this.v;
}
let t: any = getV.bind({ v: "inner" });
t = t.bind({ v: "outer" });
console.log(t());

// a proxy in the middle of the chain runs its apply trap
const px: any = new Proxy(
  function pp(a: number) {
    return a;
  }.bind(null, 3),
  {
    apply(tg: any, th: any, args: any[]) {
      return "trap:" + args.join(",");
    },
  },
);
const pb: any = px.bind(null, 4).bind(null, 5);
console.log(pb(6));

// a redefined name on a middle layer is what the next layer reads
let k: any = function kk() {
  return 7;
}.bind(null);
Object.defineProperty(k, "name", { value: "zz" });
k = k.bind(null).bind(null);
console.log(k.name, k());

// a deleted name reads Function.prototype's ""
let d: any = function dd() {
  return 8;
}.bind(null);
delete d.name;
d = d.bind(null);
console.log(JSON.stringify(d.name), d());

// a name redefined after the outer layer was made does not change it
let e: any = function ee() {}.bind(null);
const e2: any = e.bind(null);
Object.defineProperty(e, "name", { value: "later" });
console.log(e.name, e2.name);

f = null;
console.log("done");
