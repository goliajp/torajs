// Annex B §B.3.3.3 — a block's `function f` and the module's own
// `function f` share ONE global binding: §16.1.7 step 16 fills it with
// the module-top function before the first statement, and the block's
// evaluation writes the block function into it. Expected output is
// node's line for line, run as a SCRIPT (`vm.runInThisContext`): a
// `.cts` file is a CommonJS module to node and bun (module-local
// functions, and bun answers the module-top function throughout), and
// an indirect eval is eval code (its function binding is configurable,
// §9.1.1.4.18 with D = true). Before: the module-top declaration was
// dead code — every `f` meant the block function, and `f()` before the
// block answered it.

// block before the declaration
console.log("before", typeof f, f());
{ function f() { return 'inner'; } }
console.log("after", f());
function f() { return 'outer'; }
console.log("end", f(), f.name);

// declaration before the block (already right; must stay so)
function g() { return 'g-outer'; }
console.log("g-before", g());
{ function g() { return 'g-inner'; } }
console.log("g-after", g());

// the global-object property: the function's attributes, the same
// binding the bare name reads, before and after the block
var d0 = Object.getOwnPropertyDescriptor(globalThis, 'h');
console.log("h-desc-before", typeof d0.value, d0.writable, d0.enumerable, d0.configurable, globalThis.h === h);
{ function h() { return 'h-inner'; } }
var d1 = Object.getOwnPropertyDescriptor(globalThis, 'h');
console.log("h-desc-after", typeof d1.value, d1.writable, d1.enumerable, d1.configurable, globalThis.h === h, h());
function h() { return 'h-outer'; }

// switch case / if-else branches
console.log("s-before", s());
switch (1) { case 1: function s() { return 's-inner'; } }
console.log("s-after", s());
function s() { return 's-outer'; }

console.log("e-before", e());
if (false) { function e() { return 'e-then'; } } else { function e() { return 'e-else'; } }
console.log("e-after", e());
function e() { return 'e-outer'; }
