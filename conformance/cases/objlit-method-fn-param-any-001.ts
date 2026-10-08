// An object-literal method with a fn-typed parameter, reached through
// `any` (§13.3.6.1 — the call passes the argument as it is) with an arrow
// and a named function; typed member calls with a named function, an
// arrow and a forwarded fn-typed parameter keep working alongside.
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
o.k((v: number) => console.log("typed arrow", v));
function relay(cb: (v: number) => void) {
  o.k(cb);
}
relay(nf);
relay((v: number) => console.log("relayed", v));
