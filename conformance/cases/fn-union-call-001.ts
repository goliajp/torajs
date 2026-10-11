// a call through a choice among functions of different signatures runs
// the chosen one; its result is one of the members' returns and takes
// part in what follows
function inc(n: number) {
  return n + 1;
}
function label(n: number) {
  return "#" + n;
}
function echo(x: any) {
  return x;
}
const n = Number(process.argv.length > 99 ? "7" : "3");
const r = (n % 2 === 0 ? inc : label)(n);
console.log(r, typeof r, r + "!");
const f = n % 3 === 0 ? inc : label;
const v = f(n);
console.log(v, typeof v === "number" ? v * 2 : v.length);
let g = n > 5 ? label : inc;
console.log(g(10), [g(1), f(2)].join("|"));
const h = n > 1 ? inc : echo;
console.log(h(41), (n > 5 ? inc : echo)("s"));
function pick(c: boolean, k: number) {
  const p = c ? inc : label;
  return p(k);
}
const out: (number | string)[] = [];
for (let k = 0; k < 4; k++) out.push(pick(k % 2 === 0, k));
console.log(out.join(","), out.length);
function twice(c: boolean) {
  return `${(c ? inc : label)(2)}/${(c ? label : inc)(3)}`;
}
console.log(twice(true), twice(false));
