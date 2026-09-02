// 562-06 — bun prints a class object as `[class Z]`, never as the
// property bag it is. A class object IS a dynobj carrying `name` /
// `length` / `prototype` (§10.2.3 MakeConstructor), so tr's
// ordinary-object walker printed
// `{ name: "Z", prototype: Z { m: [Function: m] }, length: 0 }` —
// at every depth, inside arrays and object fields too.
class Z { m() {} }
console.log(Z);
// Static members do not open a block either.
class S { static s = 1; static t() {} }
console.log(S);
// `extends` names the superclass — the class object's own
// [[Prototype]], which is a registered class exactly when the class
// extends a user class.
class E2 extends Z {}
console.log(E2);
class E3 extends E2 {}
console.log(E3);
// A class expression is named by its binding, or by its own name.
const anon = class {};
console.log(anon);
console.log(class Named {});
// A generic class is a class object like any other.
class G2<T> { v: T | undefined; }
console.log(G2);
// A plain function keeps the function form.
function f1() {}
console.log(f1);
// Nested: no block at any depth.
console.log([Z, f1]);
console.log({ c: Z });
console.log([[E2]]);
// The prototype's `constructor` is the same class object.
console.log(Z.prototype.constructor);
// The prototype object itself still prints as an object.
console.log(Z.prototype);

// Two forms deliberately left out, both open:
//   console.log(class {});      tr `[class __ClassExpr_<n>]`, bun
//     `[class (anonymous)]` — 563-05: the desugar's synthetic name
//     leaks into `.name` itself ((class {}).name is
//     "__ClassExpr_0" where bun answers ""), so the printer is
//     honest and the bug is upstream of it.
//   console.log(Array);         tr `[Function: Array]`, bun
//     `[class Array]` — 563-06: a builtin constructor is not in the
//     class registry this reads.
