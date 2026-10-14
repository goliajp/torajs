// A function a global eval declares, read through its binding as a
// value, still receives `this` from a method call, `.call` and `new`.
eval("function f1(x){ this.a = x }");
const holder = { F: f1, a: 0 };
const viaMember = new holder.F(3);
console.log(viaMember.a, viaMember instanceof f1);
holder.F(5);
console.log(holder.a);
const target = { a: 0 };
f1.call(target, 6);
console.log(target.a);
const alias = f1;
console.log(new alias(7).a);
console.log(globalThis.f1 === f1, f1.length);
