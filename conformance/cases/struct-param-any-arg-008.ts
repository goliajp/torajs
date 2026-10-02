// A function expression that reads `this` keeps a receiver slot ahead
// of its struct parameter in the boxed adapter, so the struct slot sits
// in the second window. A fitting object reaches the typed body through
// the adapter whether the call is typed or dynamic.
type P = { x: number; y: number };
const o = {
  k: 10,
  m: function (p: P): number { return this.k + p.x; },
};
console.log(o.m({ x: 1, y: 2 }));
const g: any = o;
console.log(g.m({ x: 5, y: 6 }));
