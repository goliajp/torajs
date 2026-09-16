// RFC 20260917-plain-fn-new-target knife 3 — [[Construct]] through a
// value hands new.target to a function that reads it.

function F(this: any) { this.t = new.target === F; }
const K: any = F;
console.log(new K().t);
function G(this: any) { this.nt = new.target === G; }
console.log(Reflect.construct(G, []).nt);
class X {}
function H(this: any, a: number) { this.a = a; this.x = new.target === X; }
const hx = Reflect.construct(H, [5], X);
console.log(hx.a, hx.x, hx instanceof X);
const B: any = (F as any).bind(null);
console.log(new B().t);
const plain: any = F;
const o: any = {};
plain.call(o);
console.log(o.t);
function Ret(this: any) { return new.target; }
const R: any = Ret;
console.log(new R() === Ret, R() === undefined);
