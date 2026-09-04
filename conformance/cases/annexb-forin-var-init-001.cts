// annexB §B.3.5 — `for ( var BindingIdentifier Initializer in
// Expression ) Statement`. §14.7.5's own head binds a name and nothing
// more; this production lets it carry an initializer, evaluated once
// in the head, before the loop, and then overwritten by the first key.
//
// This file is `.cts` — the sloppy script goal. Like the rest of
// §B.3 the production "is only applied when parsing code that is not
// strict mode code", so the refusals (a module, a `"use strict"`
// prologue, a class body) are negative cases; bun accepts all three,
// so they live in test262 rather than here.

// The initializer survives when the object has no keys to overwrite it.
for (var x = 5 in {}) ;
console.log("empty", x);

// Otherwise the last key wins, and the initializer is invisible.
var seen = [];
for (var y = 5 in { a: 1, b: 2 }) seen.push(y);
console.log("keys", seen.join(","), y);

// Evaluated exactly once, in the head, before the first iteration.
var calls = 0;
function side() {
  calls++;
  return 7;
}
for (var z = side() in {}) ;
console.log("once", calls, z);

// The binding is a `var`, so it hoists out of the loop like any other.
function host() {
  for (var inner = 1 in {}) ;
  return inner;
}
console.log("hoists", host());

// The initializer reads `[~In]`: an `in` of its own needs parentheses,
// and the head's `in` is the one that ends it.
for (var p = ("a" in { a: 1 }) in {}) ;
console.log("paren-in", p);

// The plain head is unaffected.
for (var q in { m: 1 }) ;
console.log("plain", q);
