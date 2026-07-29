// ES §12.7.2 — a Unicode escape is allowed inside an IdentifierName
// but not inside the ReservedWord itself, so legality is a matter of
// POSITION: an escaped `break` as a property key is fine, an escaped
// `if` heading a statement is not. The lexer used to refuse every
// escaped reserved word outright, which took the legal positions down
// with the illegal ones. It now hands the parser a distinct token
// that the property-name positions opt into and everything else
// refuses by construction.
//
// tr already accepted the UNESCAPED keyword in all of these spots;
// only the escaped spelling was missing.

// Object-literal property key, and the member name after a dot — the
// escaped and unescaped spellings must name the SAME property.
const o: any = { bre\u0061k: 42 };
console.log("propkey", o.bre\u0061k);
console.log("same-property", o.break);

// Method definition in an object literal.
const m: any = { d\u0065lete() { return 7; } };
console.log("methoddef", m.delete());

// Class member name, including a generator member.
class C {
  cl\u0061ss() {
    return 2;
  }
  *n\u0065w() {
    yield 3;
  }
}
const c: any = new C();
console.log("classmember", c.class());
console.log("classgen", c.new().next().value);

// Destructuring property key. The BINDING may not be a reserved
// word, so — exactly as for a bare keyword — the explicit rename is
// required and supplied here.
const { bre\u0061k: bk } = { break: 9 };
console.log("destr", bk);

// NOT legal, and still refused (verified by probe, kept out of this
// fixture because a parse error is not a runnable line): the keyword
// positions — an escaped `if` / `var` / `this` — each reject naming
// the escaped word.
