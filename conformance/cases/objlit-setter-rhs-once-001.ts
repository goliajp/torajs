// §13.15.2 — `o.s = rhs` on an object-literal setter evaluates the
// right-hand side exactly once, before [[Set]] runs the setter, and the
// assignment evaluates to that value (not the setter's return, not a
// second evaluation).
let n = 0;
function side(): number {
  n++;
  console.log("SIDE", n);
  return n * 10;
}
const o = {
  _v: 0,
  get s() {
    return this._v;
  },
  set s(v: number) {
    console.log("SET", v);
    this._v = v;
  },
};
o.s = side();
console.log(o.s, n);
const r = (o.s = side());
console.log(r, o.s, n);
let k = 0;
o.s = (k++, 7);
console.log(o.s, k);

let m = 0;
function word(): string {
  m++;
  return "w" + m;
}
const t = {
  _w: "",
  set w(x: string) {
    console.log("SETW", x);
    this._w = x;
  },
  get w() {
    return this._w;
  },
};
const got = (t.w = word());
console.log(got, t.w, m);
