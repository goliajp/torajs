// Rotation 636 — `new.target` is global-code early error (§16.1.1),
// but every function form still binds it: a declaration, a class
// constructor, a function expression, an arrow nested in a function,
// and an object-literal accessor (MethodDefinition is function code).
function f() { return new.target === undefined; }
console.log("decl", f());
class C { constructor() { console.log("ctor", new.target === C); } }
new C();
const g = function () { return new.target; };
console.log("fnexpr", g());
function h() { const a = () => new.target; return a() === undefined; }
console.log("nested-arrow", h());
let seen: any = null;
const o = { get m() { seen = new.target; return 1; } };
o.m;
console.log("getter", seen);
