// A function a global eval declares, constructed through its global
// binding, sees the constructor as `new.target` and the new object as
// `this` — whether or not anything else names the function.
eval("function F(){ console.log('F', new.target === F, new.target !== undefined) }");
new F();
eval("function A(){ console.log('A', new.target !== undefined) }");
new A();
A();
eval("function C(x){ this.x = x; console.log('C', typeof new.target, new.target === C) }");
function G() {}
const r = Reflect.construct(C, [1], G);
console.log(r.x, Object.getPrototypeOf(r) === G.prototype, r instanceof C);
const c = new C(2);
console.log(c.x, c instanceof C);
function build(n) {
  return new C(n);
}
console.log(build(3).x);
eval("function V(){ const read = () => new.target; console.log('V', read() === V) }");
new V();
V();
eval("function T(){ this.q = arguments.length; console.log('T', this === globalThis) }");
const t = new T(1, 2);
console.log(t.q, t instanceof T);
eval("function P(a, b){ this.s = a + b }");
const p = new P(3, 4);
console.log(p.s, p instanceof P);
