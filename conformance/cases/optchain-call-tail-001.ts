// An optional chain that goes on past the optional link and ends in a
// call — `o?.m.call(o, x)`, `o?.a.m(x)` — short-circuits as a whole: a
// nullish `o` answers undefined and the arguments never evaluate.

class K {
  k = 10;
  m(x: number) {
    return this.k + x;
  }
}
class Box {
  a = new K();
}
const log: string[] = [];
function arg(v: number) {
  log.push("arg" + v);
  return v;
}

let o: K | null = null;
console.log(o?.m.call(o, arg(1)));
console.log(o?.m.apply(o, [arg(2)]));
o = new K();
console.log(o?.m.call(o, arg(3)));
console.log(o?.m.apply(o, [arg(4)]));

let a: any = null;
console.log(a?.m.call(a, arg(5)));
a = new K();
console.log(a?.m.call(a, arg(6)));

let b: Box | undefined = undefined;
console.log(b?.a.m(arg(7)));
b = new Box();
console.log(b?.a.m(arg(8)));

const s: string | null = null;
console.log(s?.toUpperCase.call("x"));
console.log(log.join(","));
