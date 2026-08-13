// ES §14.11 `with` — RFC 20260814 刀 4: a function written INSIDE the
// body. Sloppy goal only, so this fixture is `.cts`.
//
// A nested function resolves its free names when it is CALLED, which
// can be long after the block has been left. Nothing special is needed
// for that: the `with` binding is an ordinary block-scoped `let`, so
// the guards grown inside the function body capture it the way any
// closure captures any outer binding.
//
// What each block pins:
//   - the object is captured, not snapshotted: a property added,
//     changed or deleted AFTER the block changes what a later call
//     answers, because §9.1.1.2.1 HasBinding lives at the reference;
//   - all three body shapes — function expression, arrow, and a
//     function DECLARATION inside the block;
//   - the nested function's OWN records sit in front of the object:
//     its parameters, its `var`s and its `arguments` all shadow, and
//     only what it leaves free reaches the object;
//   - a `let` in the nested body shadows, exactly as in the with body;
//   - writes and `typeof` work through the capture too;
//   - a name declared inside the nested function must NOT shadow the
//     with body's own reference to that name (the two scopes are
//     separate, which a flat binder set would get wrong).

var o: any = { x: "object" };
var x = "outer";

var read: any = null;
var write: any = null;
var kind: any = null;
var declared: any = null;

with (o) {
  read = function (): string {
    return x;
  };
  write = function (v: string): void {
    x = v;
  };
  kind = (): string => typeof x;
  function decl(): string {
    return x;
  }
  declared = decl;
}

// Captured, and re-tested per call.
console.log(read(), declared(), kind());
o.x = "changed";
console.log(read(), declared());
write("written");
console.log(o.x, x);
delete o.x;
// The object no longer carries it, so the same closure now answers
// the outer binding — and the write above never touched it.
console.log(read(), declared(), kind());

// The nested function's own records sit in front of the object.
var byParam: any = null;
var byVar: any = null;
var byLet: any = null;
var shadowed: any = { x: "object-again" };
with (shadowed) {
  byParam = function (x: string): string {
    return x;
  };
  byVar = function (): string {
    var x = "own-var";
    return x;
  };
  byLet = function (): string {
    let x = "own-let";
    return x;
  };
}
console.log(byParam("param"), byVar(), byLet());

// `arguments` belongs to the nested function, not to the object.
var args: any = { arguments: "object-arguments" };
var seeArgs: any = null;
with (args) {
  seeArgs = function (): number {
    return arguments.length;
  };
}
console.log(seeArgs(1, 2, 3));

// A name bound only INSIDE the nested function must not shadow the
// with body's own reference to it.
var scoped: any = { y: "object-y" };
var y = "outer-y";
var inner: any = null;
with (scoped) {
  inner = function (): string {
    let y = "inner-y";
    return y;
  };
  console.log(y);
}
console.log(inner(), y);

// Two functions deep, both capturing the same binding.
var deep: any = null;
var two: any = { z: "object-z" };
var z = "outer-z";
with (two) {
  deep = function (): any {
    return function (): string {
      return z;
    };
  };
}
console.log(deep()(), z);
