// `arguments` in a body whose last parameter is a rest parameter.
// §10.4.4 builds the object from the call's argument list: past the
// fixed parameters, position i is an element of the rest array, and a
// call that passes fewer than the fixed parameters has a shorter list.
class P {
  out: string;
  constructor(a, b, ...r) {
    const xs = [...arguments];
    let s = "";
    for (let i = 0; i < arguments.length; i++) s += String(arguments[i]) + ",";
    this.out = arguments.length + "|" + xs.length + "|" + s + "|" + r.length + "|" + arguments[2];
  }
}
console.log(new P().out);
console.log(new P(1).out);
console.log(new P(1, 2).out);
console.log(new P(1, 2, 3, 4).out);
class D extends P {}
console.log(new D(9, 8, 7).out);
class E extends P {
  constructor(x) {
    super(x, "e", "f");
  }
}
console.log(new E(5).out);

class Q {
  n: string;
  constructor(a, ...r) {
    this.n = arguments.length + ":" + arguments[1] + ":" + r.length;
  }
}
console.log(new Q(1, 2, 3).n, new Q(1).n);

class T {
  n: string;
  constructor(a: number = 10, ...r: number[]) {
    this.n = arguments.length + ":" + a + ":" + arguments[1] + ":" + r.join("/");
  }
}
console.log(new T().n, new T(1).n, new T(1, 2, 3).n);

function f(a, b, ...r) {
  const all = arguments;
  return all.length + ":" + [...arguments].length + ":" + arguments[3];
}
console.log(f(1), f(1, 2, 3, 4));
function g(a, ...r) {
  return arguments[0] + ":" + arguments[2] + ":" + [...arguments].length;
}
const h: any = g;
console.log(h(1, 2, 3), h());
console.log(g.call(null, 4, 5, 6), g.apply(null, [9]));
const o = { g };
console.log(o.g(3, 4, 5));
function sum(a: number, ...r: number[]) {
  let t = 0;
  for (const v of arguments) t += v;
  return t;
}
console.log(sum(1), sum(1, 2, 3));
class S {
  static m(a, ...r) {
    return arguments.length + ":" + arguments[1];
  }
}
console.log(S.m(1), S.m(1, 2, 3));
