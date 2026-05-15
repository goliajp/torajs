// T-37 — Destructuring catch parameter (`catch ({ ... }) {}` /
// `catch ([ ... ]) {}`). ES2018+ BindingPattern syntax. test262
// annexB cases use this shape (e.g. `catch ({ f })`) as a syntactic
// check that the throw happened — the body doesn't actually
// reference the destructured names.
//
// Implementation: parser skips the inner pattern syntactically
// (matching brace/bracket nesting) and binds an anonymous synthetic
// `__catch_destr_<pos>` so the rest of the catch parses. Body
// references to the destructured names will fail with `unknown
// identifier` later — that's a follow-up if real binding is needed
// (T-37-followup).

let saw = "";

// Object pattern.
try {
  throw { x: 1, y: 2 };
} catch ({ x, y }) {
  saw = "obj";
}
console.log(saw);  // obj

// Array pattern.
saw = "";
try {
  throw [1, 2, 3];
} catch ([a, b]) {
  saw = "arr";
}
console.log(saw);  // arr

// Nested object pattern.
saw = "";
try {
  throw { outer: { inner: 7 } };
} catch ({ outer }) {
  saw = "nested";
}
console.log(saw);  // nested

// Catch + finally + destructure.
saw = "";
let after = false;
try {
  throw { v: 9 };
} catch ({ v }) {
  saw = "v-caught";
} finally {
  after = true;
}
console.log(saw);   // v-caught
console.log(after); // true
