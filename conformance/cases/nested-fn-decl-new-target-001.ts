// §B.3.3 — a `function` declaration nested inside a function body is
// lifted to top level under a mangled name, and every reference to it
// in the enclosing body is rewritten to that name. The rewrite walks
// operands, and three expression shapes carried an operand it never
// descended into, so a reference sitting in one of them kept the
// pre-lift spelling: the callee of a dynamic `new` (which is what a
// non-class constructor becomes), a cast, and `delete`.

function outer(): boolean {
  function Foo(): void {}
  const t = function () {
    return new (Foo as any)();
  };
  return (t() as any).constructor === Foo;
}
console.log(outer());

function direct(): boolean {
  function Bar(): void {}
  const made: any = new (Bar as any)();
  return made.constructor === Bar;
}
console.log(direct());

function viaArrow(): boolean {
  function Baz(): void {}
  const t = () => new (Baz as any)();
  return (t() as any).constructor === Baz;
}
console.log(viaArrow());

// The lifted declaration is an ordinary constructor: instances of it
// are instances of nothing else.
function ownInstance(): boolean {
  function Qux(): void {}
  const made: any = new (Qux as any)();
  return made instanceof Qux && !(made instanceof Error);
}
console.log(ownInstance());

// The name still resolves in call position and under a cast.
function called(): number {
  function twice(n: number): number {
    return n * 2;
  }
  const alias: any = twice as any;
  return twice(21) + alias(0);
}
console.log(called());

// Two sibling declarations in one body, each reached through a
// different one of the three shapes.
function siblings(): boolean {
  function A(): void {}
  function B(): void {}
  const a: any = new (A as any)();
  const b = function () {
    return new (B as any)();
  };
  return a.constructor === A && (b() as any).constructor === B;
}
console.log(siblings());
