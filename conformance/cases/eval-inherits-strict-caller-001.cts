// §11.2.2's other two sources of strictness, under the SLOPPY script
// goal — the goal says nothing here, so what a direct eval inherits is
// whatever the calling code made itself: a `"use strict"` prologue on
// the program, and a class body (§10.2.1 makes all class code strict
// whatever encloses it).
"use strict";

// Program prologue.
try {
  eval("var yield = 1");
  console.log("prologue-yield ran");
} catch (e) {
  console.log("prologue-yield", SyntaxError.prototype.isPrototypeOf(e));
}
try {
  eval("function f(a, a){}");
  console.log("prologue-dup ran");
} catch (e) {
  console.log("prologue-dup", SyntaxError.prototype.isPrototypeOf(e));
}
// Strictness is inherited by nested functions and never flips back.
function nested() {
  try {
    eval("if (true) function g(){}");
    console.log("nested-annexb ran");
  } catch (e) {
    console.log("nested-annexb", SyntaxError.prototype.isPrototypeOf(e));
  }
}
nested();

// Class code.
class K {
  m() {
    try {
      eval("var static = 1");
      console.log("class-reserved ran");
    } catch (e) {
      console.log("class-reserved", SyntaxError.prototype.isPrototypeOf(e));
    }
  }
}
new K().m();

console.log("done", eval("1 + 1"));
