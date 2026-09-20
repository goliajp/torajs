// Annex-B §B.2.1.1 / §B.2.1.2 — `escape` / `unescape` read as VALUES.
// The direct call has lowered through the torajs-str kernels since the
// §19.2.6 URI quartet landed; only the value face was missing, so
// `escape.length` was an unknown ident while `escape("a b")` worked.
console.log(escape("a b+/~"));
console.log(unescape("a%20b%2B"));

console.log(typeof escape, typeof unescape);
console.log(escape.length, unescape.length);
console.log(escape.name, unescape.name);

// Detached: the cell's call face runs the same kernel.
const e = escape;
const u = unescape;
console.log(e("é"), u("%E9"));
console.log(e.call(null, "x y"), u.apply(null, ["x%20y"]));

// Neither is a constructor (§B.2.1.1 has no [[Construct]]).
const asCtor: any = escape;
try {
  new asCtor("x");
  console.log("no throw");
} catch (err) {
  console.log((err as Error).constructor.name);
}

// ToString first, so a non-string argument coerces.
console.log(escape(1), escape(null), escape(undefined));
console.log(unescape(123));
