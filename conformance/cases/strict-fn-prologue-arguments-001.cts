// §11.2.2 — a function carrying its own `"use strict"` directive
// prologue is strict even when the surrounding Script is sloppy. That
// makes strictness a per-FUNCTION question, and the shape of
// `arguments` is decided by it: §10.4.4 mints the mapped object for a
// sloppy function and §10.4.5 the unmapped one for a strict function.
//
// Asking the PROGRAM instead gets the outer answer for every function
// in the file — mapped parameters and a data `callee` — and six
// test262 %ThrowTypeError% cases reach that intrinsic through exactly
// this door.
//
// bun is not an oracle here, in two separate ways: reading a `.cts` it
// drops a function-level `"use strict"` entirely (both functions come
// back mapped), and even on a `.js` it reports the SLOPPY function as
// unmapped. node agrees with the spec line by line, so this checks
// against a `.expected`.

function sloppyMapping(a) {
  a = 1;
  return arguments[0];
}
console.log(sloppyMapping(0));

function strictMapping(a) {
  "use strict";
  a = 1;
  return arguments[0];
}
console.log(strictMapping(0));

// The strict function's `callee` is the shared %ThrowTypeError%
// accessor, not a data property.
function strictArgs() {
  "use strict";
  return arguments;
}
var strictDesc = Object.getOwnPropertyDescriptor(strictArgs(), "callee");
console.log(typeof strictDesc.get, typeof strictDesc.set);
console.log(strictDesc.get === strictDesc.set);
console.log("value" in strictDesc);
console.log(Object.getPrototypeOf(strictDesc.get) === Function.prototype);
console.log(Object.prototype.hasOwnProperty.call(strictDesc.get, "arguments"));
console.log(Object.prototype.hasOwnProperty.call(strictDesc.get, "caller"));

// And the sloppy one's is a data property holding the function.
function sloppyArgs() {
  return arguments;
}
var sloppyDesc = Object.getOwnPropertyDescriptor(sloppyArgs(), "callee");
console.log(typeof sloppyDesc.value, sloppyDesc.value === sloppyArgs);
