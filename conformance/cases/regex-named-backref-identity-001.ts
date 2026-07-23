// annexB §B.1.4 — outside u/v mode, a pattern with NO named
// capture group anywhere treats `\k` as a literal `k` and the
// trailing bytes (`<name>` if any) reparse as literals. Pre-fix
// `parse_named_backref` set_err on non-`<` follow / missing group,
// poisoning the whole regex. Sister to the `\xHH` identity fix
// (chunk 3ffe1f44). test262
// `annexB/built-ins/RegExp/named-groups/non-unicode-malformed.js`
// exercises this.

// `\k<x>` with NO named group anywhere → literal "k<x>".
console.log(/\k<x>/.test("k<x>"));
// true

// `\k<x>` with a defined named group → real backref.
console.log(/(?<x>a)\k<x>/.test("aa"));
// true

// `\k` alone (no `<...>`) with no named groups → literal "k".
console.log(/\ka/.test("ka"));
// true

// `\k<>` with no named groups → literal "k<>".
console.log(/\k<>/.test("k<>"));
// true
