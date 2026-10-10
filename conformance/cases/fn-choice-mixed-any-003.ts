// a named function joined into the any lane by `??` / `||` / `&&` keeps
// its one fn object alive across many owned joins
function inc(n: number): number { return n + 1; }
const objs: any = {};
let s = 0;
for (let i = 0; i < 100000; i++) {
  const r = objs.none ?? inc;
  s += r(1);
}
const r2 = objs.none ?? inc;
console.log(s, r2 === inc);
const g: any = inc;
console.log(g(41), inc(1));
(inc as any).tag = 7;
console.log((inc as any).tag, (r2 as any).tag);
for (let i = 0; i < 1000; i++) {
  objs.none ?? inc;
  const t = objs.none || inc;
  s += t(2);
  const u = (i > 0 && inc) || objs.none;
  if (i > 0) s += u(3);
}
const h: any = objs.none ?? inc;
console.log(s, h(5), h === inc, (h as any).tag);
