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

// An anonymous class expression prints `[class (anonymous)]` — see
// anonymous-class-name-001 (563-05, closed: the synthetic name no
// longer reaches `.name`, this printer, or an instance's prefix).
console.log(class {});

// One form deliberately left out, still open:
//   console.log(Array);         tr `[Function: Array]`, bun
//     `[class Array]` — 563-06: a builtin constructor is not in the
//     class registry this reads. bun prints `[class X]` for nearly
//     every builtin (`[class TypeError extends Error]`,
//     `[class Uint8Array extends TypedArray]`) and
//     `[Function: Promise]` for exactly one.
