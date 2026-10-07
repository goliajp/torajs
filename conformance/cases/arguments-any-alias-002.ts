// More targets of an `any`-typed alias that reads `arguments`: a
// generator, an async function, a function reading `this`, and
// length-only bodies whose typed parameters are read or never read.
function* g(a) {
  yield arguments.length;
  yield a;
}
const gv: any = g;
console.log([...gv()].join(","), [...gv(1)].join(","), [...gv(1, 2, 3)].join(","));

function t(a) {
  "use strict";
  return arguments.length + ":" + a + ":" + typeof this;
}
const tv: any = t;
console.log(tv(), tv(1), tv(1, 2, 3));

async function af(a) {
  return arguments.length + ":" + a;
}
const afv: any = af;
afv().then((x) => console.log("async " + x));
afv(1, 2).then((x) => console.log("async " + x));

function s(a: string, b: number) {
  return arguments.length;
}
const sv: any = s;
console.log(sv(), sv("x"), sv(undefined, undefined, 3));

function w(a: boolean) {
  if (arguments.length > 0) return arguments.length + ":" + a;
  return "none";
}
const wv: any = w;
console.log(wv(), wv(true), wv(0, 1));

function m2(a: number) {
  const f = () => 5;
  return arguments.length + ":" + a + ":" + f();
}
const m2v: any = m2;
console.log(m2v(), m2v(1), m2v(1, 2));
