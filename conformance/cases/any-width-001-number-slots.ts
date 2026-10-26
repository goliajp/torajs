// An `any` value written into a slot typed `number` keeps its
// fraction: a parameter, a reassigned binding, a method or a closure
// return the context types `number`, and a callback handed one.

function h(n: number) {
  return n + 1;
}
class S {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function run(raw: any) {
  const x: number = raw.y;
  console.log(x + 1);
  let z: number = 0;
  z = raw.y;
  console.log(z + 1);
  console.log(h(raw.y));
  console.log(new S(raw).m() + 1);
  const f: () => number = () => raw.y;
  console.log(f() + 1);
  const g = (cb: () => number) => cb() + 1;
  console.log(g(() => raw.y));
  console.log([1, 2].map((): number => raw.y)[0] + 1);
  console.log(h(raw.n), h(raw.big));
}
run(JSON.parse('{"x":2.5,"y":3.5,"n":-0.25,"big":9007199254740993}'));
const r: any = JSON.parse('{"y":0.5}');
console.log(h(r.y));
