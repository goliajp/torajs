// An own `exec` defined through a writing Object / Reflect static that
// the program reads under a computed key rather than calling it by its
// plain spelling. RegExpExec (§22.2.7.1) calls the own `exec`, so the
// typed String methods find no match; a RegExp no write reached still
// matches.
const k = process.argv.length > 99 ? "x" : "exec";
const r1 = /a/;
(Object as any)["assign"](r1, { exec: () => null });
console.log("xaxa".replace(r1, "b"));
const r2 = /[a]/;
(Reflect as any)["set"](r2, k, () => null);
console.log("xaxa".replace(r2, "b"), "xaxa".search(r2));
const r4 = /(a)/;
console.log("xaxa".replace(r4, "b"), "xaxa".search(r4));
