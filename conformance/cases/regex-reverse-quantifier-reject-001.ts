// ES §22.2.1.1 Term / QuantifierPrefix Static Semantics —
// `{n,m}` requires `n <= m`. bun/JSC throw SyntaxError from the
// RegExp constructor unconditionally (u and non-u). Pre-fix tr
// accepted the reversed pair and produced a never-match matcher.

// Reversed bounds throw
try { new RegExp("a{2,1}", ""); console.log("no-throw"); }
catch (e: any) { console.log("2,1:", e.name); }

try { new RegExp("a{2,1}", "u"); console.log("no-throw u"); }
catch (e: any) { console.log("2,1-u:", e.name); }

try { new RegExp("(ab){10,5}", ""); console.log("no-throw group"); }
catch (e: any) { console.log("10,5-group:", e.name); }

// Boundary: n == m is fine (not reversed)
console.log("equal:", new RegExp("a{2,2}", "").test("aa"));

// Normal ordered {n,m} works
console.log("normal:", new RegExp("a{1,3}", "").test("aa"));

// Open-ended {n,} works
console.log("open:", new RegExp("a{2,}", "").test("aaaa"));

// Exact {n} works
console.log("exact:", new RegExp("a{3}", "").test("aaa"));
