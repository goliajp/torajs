// a default of any declared type runs in the callee: a function value has
// no call site that could pad it (ECMA-262 §10.2.11)
class P {
  constructor(public x: number) {}
}
interface Pair {
  a: number;
  b: string;
}
function local(k: number[]) {
  function f(y: number[] = k) {
    return y.length;
  }
  return f();
}
console.log("array default, local call:", local([1, 2, 3]));

function escaping(k: number[], p: P, s: string[], o: Pair) {
  function f(y: number[] = k) {
    return y.length;
  }
  function g(q: P = p) {
    return q.x;
  }
  function h(t: string[] = s, u: number | string = k.length) {
    return t.join("+") + u;
  }
  function i(v: Pair = o, m: Map<string, number> = new Map([["z", k[0]]])) {
    return v.a + v.b + m.get("z");
  }
  return [f, g, h, i] as const;
}
const [f, g, h, i] = escaping([1, 2, 3], new P(5), ["a", "b"], { a: 1, b: "s" });
const u: any = undefined;
console.log("escaping:", f(), f([9]), f(u), g(), g(new P(8)), h(), h(["z"], "w"), i());

const arrow = (y: number[] = [1, 2]) => y.length;
const fnExpr = function (q: P = new P(3)) {
  return q.x;
};
function arrowCapture(k: number[]) {
  return (y: number[] = k) => y.length;
}
console.log("values:", arrow(), arrow([5]), arrow(u), fnExpr(), fnExpr(u));
console.log("arrow capture:", arrowCapture([1, 2, 3])(), arrowCapture([1])(u));

function gen<T>(a: T, b: T = a) {
  return b;
}
console.log("type parameter slot:", gen(3), gen("x", "y"));

function shapes(k: number) {
  function j(m: number[] = [k], n: string = `${k}`, o: () => number = () => k * 2) {
    return m[0] + n + o();
  }
  function q(c: P = new P(k), t: string = typeof k, z: number = c?.x ?? 0) {
    return c.x + t + z;
  }
  return [j, q] as const;
}
const [j, q] = shapes(5);
console.log("other shapes:", j(), j([1], "n", () => 0), q(), q(new P(2)));
