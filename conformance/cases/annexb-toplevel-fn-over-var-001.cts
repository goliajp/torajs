// A `var f = e` anywhere in the module over a `function f`: one
// binding (§16.1.7 step 8 / 10), holding the function from the first
// statement on and whatever the `var`'s initializer stores once it
// runs. Two shapes reach it — a user's own `var`, and the `var f =
// <block binding>` the capturing lane leaves behind for a block
// function that reads a module-top `let` (Annex B §B.3.3.3 again, the
// variant the lift's block-nested scan cannot see). Expected output is
// node's line for line, run as a Script (`vm.runInThisContext`; a
// `.cts` is a CommonJS module to node itself). Before: the var hoist
// kept a prelude slot for the name, and the slot shadowed the function
// until the `var` ran — `f()` before it was "not a function".

// block function capturing a module-top `let`, then the declaration
let y = 'cap';
console.log("cap-before", f());
{ function f() { return 'inner-' + y; } }
console.log("cap-after", f());
function f() { return 'outer'; }

// a user's `var f = e` after the declaration
console.log("var-before", typeof g, g());
var g = 2;
console.log("var-after", typeof g, g);
function g() { return 1; }
console.log("var-end", g);

// the `var` in a block; the property is the function's binding
console.log("blk-before", h());
{ var h = 3; }
console.log("blk-after", h);
function h() { return 'h'; }
var d = Object.getOwnPropertyDescriptor(globalThis, 'h');
console.log("blk-desc", d.writable, d.enumerable, d.configurable, globalThis.h === h);

// init-less `var` re-declares the function's binding and changes nothing
var k;
console.log("noinit", typeof k, k());
function k() { return 'k'; }

// the `var` before the declaration in text: the initializer runs first
var m = 1;
function m() {}
console.log("var-first", m);

// a user's own `__`-prefixed name is not a compiler-minted one
// (sputnik's `__decl`): the `var` over it takes the same binding
console.log("dunder-before", typeof __decl);
var __decl = 1;
console.log("dunder-after", __decl);
function __decl() { return 1; }
