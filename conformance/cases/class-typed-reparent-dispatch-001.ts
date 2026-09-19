// A class-typed receiver that was re-parented answers its methods,
// getters and instanceof from its user [[Prototype]], also through an
// `any` alias and inside a function typed on the original class.
class C { m() { return "C.m"; } get g() { return "C.g"; } x = 1; }
class D { m() { return "D.m"; } get g() { return "D.g"; } }
class E extends C { m() { return "E.m"; } }
const c = new C();
Object.setPrototypeOf(c, D.prototype);
console.log(c.m(), c.g, c.x, c instanceof D, c instanceof C);
function call(k: C) { return k.m() + "/" + k.g; }
console.log(call(c), call(new C()), call(new E()));
const arr: C[] = [new C(), c, new E()];
console.log(arr.map((k) => k.m()).join(","));

const c2 = new C();
const alias: any = c2;
Object.setPrototypeOf(alias, E.prototype);
console.log(c2.m(), c2 instanceof E, c2 instanceof C);
Object.setPrototypeOf(c2, C.prototype);
console.log(c2.m(), c2.g, c2 instanceof E);
