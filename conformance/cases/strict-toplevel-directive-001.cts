// §11.2.2 — global code that opens with a Use Strict Directive IS
// strict mode code, and everything nested inherits it. The visible
// consequence a byte-compare fixture can state is §10.2.1.2 step 5:
// a plain function called with no receiver sees `this === undefined`
// under strict, where sloppy code would hand it the global object.
"use strict";

function detached() {
  return typeof this;
}
console.log(detached());

function outer() {
  function inner() {
    return this === undefined;
  }
  return inner();
}
console.log(outer());

// An arrow's `this` is lexical either way, so it still reports the
// module-level receiver rather than a fresh binding.
const arrow = () => typeof this;
console.log(arrow());

// The directive only opens the prologue — a string expression after
// real code is an ordinary expression, not a second directive.
const s = "use strict";
console.log(s.length);
