// A nested class reading a name that a scope between it and the top
// level rebinds reads that binding, not the top-level one of the same
// name.
const k = 1;
function param(k: number) { class D { m() { return k } } return new D().m() }
function local(x: number) { let k = x * 2; class D { m() { return k } } return new D().m() }
function caught(x: number) { try { throw x + 1 } catch (k) { class D { m() { return k } } return new D().m() } }
function forLet(x: number) {
  let r = 0;
  for (let k = x; k < x + 1; k++) { class D { m() { return k } } r = new D().m() }
  return r;
}
function forOf(x: number) {
  let r = 0;
  for (const k of [x, x + 5]) { class D { m() { return k } } r = new D().m() }
  return r;
}
function twoDeep(k: number) { function inner() { class D { m() { return k } } return new D().m() } return inner() }
function field(k: number) { class D { x = k; } return new D().x }
function staticField(k: number) { class D { static s = k; } return D.s }
function staticBlock(k: number) { class D { static s = 0; static { D.s = k } } return D.s }
function varInBlock(x: number) { { var k = x + 4; } class D { m() { return k } } return new D().m() }
function letAfter(x: number) { class D { m() { return k } } const k = x + 7; return new D().m() }
console.log(param(6), local(6), caught(6), forLet(6), forOf(6), twoDeep(6));
console.log(field(6), staticField(6), staticBlock(6), varInBlock(6), letAfter(6), k);

class Base { v() { return 1 } }
function extendsParam(Base: any) { class D extends Base { } return new D().v() }
const key = "top";
function computedKey(key: string) {
  class D { [key]() { return 7 } }
  return Object.getOwnPropertyNames(D.prototype).join(",");
}
console.log(extendsParam(class { v() { return 2 } }), computedKey("loc"), new Base().v(), key);

const arrowParam = (k: number) => { class D { m() { return k } } return new D().m() };
const arrowLocal = (x: number) => { const k = x + 2; class D { m() { return k } } return new D().m() };
function arrowInFn(k: number) { const f = () => { class D { m() { return k } } return new D().m() }; return f() }
function callback(k: number) { return [1].map(() => { class D { m() { return k } } return new D().m() })[0] }
function arrowTwoDeep(k: number) { return (() => () => { class D { m() { return k } } return new D().m() })()() }
console.log(arrowParam(6), arrowLocal(6), arrowInFn(6), callback(6), arrowTwoDeep(6));

// nothing rebinds the name: the class reads the top-level binding
const t = 3;
function plain(x: number) { class D { m() { return t + x } } return new D().m() }
function sibling(x: number) { class A { v() { return x } } class B extends A { } return new B().v() }
console.log(plain(4), sibling(5));
