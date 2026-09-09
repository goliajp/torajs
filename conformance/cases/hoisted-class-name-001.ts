// A class declared inside a function body is hoisted to the top
// level, and when its name collides with another hoisted class or a
// top-level binding it is renamed `__hc<N>_<name>` for the program's
// bindings. That rename is the hoist's, not the program's: `.name`,
// the `[class X]` print and an instance's `X { … }` prefix keep the
// source spelling. Before this `.name` answered `__hc0_D`.
class C { tag = "top"; }
function a() { class C { tag = "a"; } return C; }
function b() { class C { tag = "b"; static s = 1; } return C; }
const c = () => { class D { x = 1; } Object.freeze(D); return D; };
const d = () => { class D { y = 2; } return D; };
const CA = a(), CB = b(), DC = c(), DD = d();
console.log(C.name, CA.name, CB.name, DC.name, DD.name);
console.log(new C().constructor.name, new CA().constructor.name, new DD().constructor.name);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(DC, "name")));
console.log(`${CB.name}!`, CB.s);
console.log(CA);
console.log(new CA());
console.log(new DD());
console.log(new CB());
console.log(CA === CB, CA === C, new CA() instanceof CA, new CA() instanceof C);
function e() { class E {} return E.name; }
function f() { class E {} return E.name; }
console.log(e(), f());
