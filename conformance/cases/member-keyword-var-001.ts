// Rotation 204 — `var` joins keyword_property_name: it was the one
// keyword with its own token missing from the centralized table
// (`in` / `with` / `enum` lex as Ident and always worked), so
// `obj.var` was a parse error. Per §12.7.6 IdentifierName the full
// reserved-word list is legal in property-name positions.

let t = { a: 1 };
t.var = "var";
console.log(t.hasOwnProperty("var"));
console.log(t.var);

// object-literal field position (same centralized table)
let o = { var: 7, catch: 8 };
console.log(o.var + o.catch);

// member read/write round-trip alongside neighbours that ride the
// Ident arm
t.in = "in";
t.with = "with";
console.log(t.in, t.with, t.var);
