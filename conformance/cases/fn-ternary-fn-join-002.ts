// a conditional expression choosing between named functions of
// different signatures is their union: whichever function is chosen
// is the value, wherever the choice is spelled and wherever the value
// goes — called in place, bound, returned, passed to an `any`
// parameter, stored in an array or an object, compared, called in a
// hot loop
function inc(n: number) {
  return n + 1;
}
function label(n: number) {
  return "#" + n;
}
function twice(n: number, m: number = 2) {
  return n * m;
}
let i = 3;
console.log((i % 2 === 0 ? inc : label)(i));
const f = i % 3 === 0 ? inc : label;
console.log(f(i));
function pick(c: boolean) {
  const g = c ? inc : label;
  return g(4);
}
console.log(pick(true), pick(false));
function app(g: any, x: number) {
  return g(x);
}
console.log(app(i > 2 ? inc : label, 5), app(i > 5 ? inc : label, 5));
const fs = [i > 2 ? inc : label, i > 5 ? inc : label];
console.log(fs.length, fs[0](1), fs[1](1));
function choose(c: boolean) {
  return c ? inc : label;
}
console.log(choose(true)(10), choose(false)(10));
const h = choose(i === 3);
console.log(h(7), typeof h, h === inc, choose(false) === label);
const three = i > 4 ? inc : i > 2 ? label : twice;
console.log(three(5), three.name, three.length);
const t = i > 2 ? twice : inc;
console.log(t(4));
const o = { pick: i > 2 ? label : inc };
console.log(o.pick(9));
let m = i > 2 ? inc : label;
console.log(m(1));
m = i > 2 ? label : inc;
console.log(m(1));
console.log([inc, label].indexOf(i > 2 ? label : inc));
let sum = 0;
let chars = 0;
for (let k = 0; k < 2000000; k++) {
  const r = (k % 3 === 0 ? label : inc)(k);
  if (typeof r === "number") sum += r;
  else chars += r.length;
}
console.log(sum, chars);
