// 605-03 taught the `new` census that a `function` declaration shadows
// a class of the same name. It recorded the other binding kinds as a
// deliberate non-gap: "a `let` / `const` / parameter binds a value
// rather than a constructor, and a `new` through one is already the
// dynamic route." Measured, that was wrong — they took the class's
// factory exactly as the `function` case did, and the program got the
// class's instance with its fields attached.
//
// The three kinds do not share a scope, which is the whole difficulty:
// a `function` (and, per Annex B §B.3.3, a block-nested one) belongs to
// the function scope, a parameter belongs to it too, but a `let` /
// `const` belongs to its BLOCK.

class W {
  tag: number = 1;
}

function viaConst(): void {
  const W: any = function (): void {};
  const x: any = new W();
  console.log("const", x.constructor === W, x.tag);
}
viaConst();

function viaLet(): void {
  let W: any = function (): void {};
  const x: any = new W();
  console.log("let", x.constructor === W, x.tag);
}
viaLet();

function viaParam(W: any): void {
  const x: any = new W();
  console.log("param", x.constructor === W, x.tag);
}
viaParam(function (): void {});

// A `const` inside a nested block shadows there too.
//
// The other half of that boundary — a `new W()` written AFTER the
// block, which must mean the class again — is not exercised here: it
// hits a separate, pre-existing bug where a block-scoped `const W`
// makes the class unreachable for the rest of the body
// (`ReferenceError: W is not defined`). Measured by reverting only
// this census file: that error is identical before and after this cut,
// while `inner` goes from the class's instance to the const's.
// Recorded, not fixed here.
function blockScoped(): void {
  {
    const W: any = function (): void {};
    const inner: any = new W();
    console.log("inner", inner.constructor === W, inner.tag);
  }
}
blockScoped();

// A `var` is function-scoped, so it shadows the whole body.
function viaVar(): void {
  var W: any = function (): void {};
  const x: any = new W();
  console.log("var", x.constructor === W, x.tag);
}
viaVar();

// Controls: unshadowed `new` still reaches the class, and the
// `function` form 605-03 fixed keeps working.
const plain: any = new W();
console.log("plain", plain.constructor === W, plain.tag);

function viaFnDecl(): void {
  function W(): void {}
  const x: any = new (W as any)();
  console.log("fndecl", x.constructor === W, x.tag);
}
viaFnDecl();
