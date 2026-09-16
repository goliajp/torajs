// RFC 20260917-plain-fn-new-target knife 2 — a function expression
// reading new.target: undefined under a call, the constructor under new.

const E2: any = function () { return new.target; };
console.log(new E2() === E2, E2() === undefined);

let E3: any;
E3 = function (this: any, x: number) { this.x = x; this.t = new.target === E3; };
const e3 = new E3(4);
console.log(e3.x, e3.t);

function outer(k: number) {
  const E: any = function (this: any, x: number) {
    const inner = () => typeof new.target;
    this.v = x + k;
    this.nt = inner();
  };
  const e = new E(1);
  const o: any = {};
  E.call(o, 2);
  return [e.v, e.nt, o.v, o.nt].join();
}
console.log(outer(10));
const W: any = function (a: number, b: number) { return new.target ? "ctor" : a + b; };
console.log(W(1, 2), typeof new W(1, 2), [5].map((v) => W(v, 1)).join());
console.log(Reflect.construct(W, [1, 2]) instanceof W);
