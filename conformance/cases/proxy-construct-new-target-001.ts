// §10.5.13 — a Proxy's [[Construct]] passes newTarget to its trap, and
// a trap-less forward hands the same newTarget to the target; Reflect
// .construct reaches a Proxy target.
let seen: any;
const Inner: any = new Proxy(function () { throw new Error("target called"); }, {
  construct(_t: any, args: any[], newTarget: any) {
    seen = newTarget;
    return { sum: args[0] + args[1] };
  },
});
const Outer: any = new Proxy(Inner, {});
const NT = function () {};
const a: any = Reflect.construct(Outer, [3, 4], NT);
console.log("reflect forward", a.sum, seen === NT);
const b: any = new Outer(1, 2);
console.log("new forward", b.sum, seen === Outer);
function Base(this: any, x: number) { this.x = x; }
// Bound through an `any` slot first: the direct `new Proxy(Base, {})`
// spelling loses `this` for a function declaration (a separate gap).
const BaseAny: any = Base;
const PB: any = new Proxy(BaseAny, {});
const c: any = new PB(5);
console.log("ordinary target", c.x, c instanceof BaseAny);
function Other() {}
(Other as any).prototype.tag = "other";
const d: any = Reflect.construct(PB, [6], Other);
console.log("newTarget prototype", d.x, d.tag, Object.getPrototypeOf(d) === Other.prototype);
class K { v = 9; }
const PK: any = new Proxy(K, {});
console.log("class target", new PK().v);
