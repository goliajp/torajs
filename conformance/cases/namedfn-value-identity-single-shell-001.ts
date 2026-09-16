// A `this`-reading declaration is ONE value: reaching it through a
// value binding, a Proxy, or Reflect must find the same function
// object, with the same prototype, and must not drop the receiver.
function Base(this: any, n: number): void {
  this.n = n;
}
const B: any = Base;
console.log("identity", B === Base);
console.log("prototype", B.prototype === Base.prototype);
const viaB: any = new B(5);
const viaDirect: any = new Base(6);
console.log("new", viaB.n, viaDirect.n);
console.log("instanceof", viaB instanceof Base, viaDirect instanceof B);
(Base as any).prototype.describe = function (this: any): string {
  return "n=" + this.n;
};
console.log("method through both", viaB.describe(), (new B(9) as any).describe());
const constructed: any = Reflect.construct(Base, [7]);
console.log("reflect construct", constructed.n);
const applied: any = {};
Reflect.apply(Base, applied, [8]);
console.log("reflect apply", applied.n);
const P: any = new Proxy(Base, {});
console.log("proxy construct", (new P(10) as any).n);
function plain(x: number): number {
  return x + 1;
}
const p: any = plain;
console.log("this-free unchanged", p === plain, p(1));
