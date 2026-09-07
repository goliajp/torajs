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

// The scope boundary, both halves: a `const` inside a nested block
// shadows there, and a `new W()` written after the block means the
// class again. (The second half was a separate pre-existing bug when
// this fixture was first written — a block-scoped `const W` made the
// class unreachable for the rest of the body — fixed in the next cut.)
function blockScoped(): void {
  {
    const W: any = function (): void {};
    const inner: any = new W();
    console.log("inner", inner.constructor === W, inner.tag);
  }
  const outer: any = new W();
  console.log("outer", outer.constructor === W, outer.tag);
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
