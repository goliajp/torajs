// A face-mismatched MUTABLE fn-typed slot spelled `var`.
//
// The mismatch census routes such a binding through the boxed dual
// entry, because the bare closure-call lane fires a `call_indirect`
// shaped by the slot's ANNOTATION and a stored callee declaring more
// params would read an argument register the caller never filled.
// `var` says nothing about either half of that, but the census only
// walked `let` — so this spelling read the unfilled register and
// printed garbage for the default-initialized parameter.

function ga() { console.log("ga"); }
function gb(p = 5) { console.log("gb", p); }

var slot: () => void = ga;
slot();
slot = gb;
slot();

// The init site is under the same rule as the assign site.
var seeded: () => void = gb;
seeded();
