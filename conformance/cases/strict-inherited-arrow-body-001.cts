// §11.2.2 — an arrow body inside strict code is strict code. The
// per-function question §13.15.2 asks (which [[Set]] kernel does this
// assignment take) is answered off the body's directive prologue, so
// the parser writes the inherited strictness into an arrow body the
// same way it writes it into a function expression's.
//
// Every arrow below sits in a `"use strict"` function inside a sloppy
// script: a refused [[Set]] must throw, and the sloppy arrow at the
// end must still discard the refusal.
//
// Oracle is node, pinned in `.expected`. bun answers neither goal
// here: it runs a `.cts` entry with no strictness at all (even an own
// `"use strict"` in a function is discarded), and a `.js` entry as
// strict throughout (the top-level sloppy arrow throws too). node's
// CommonJS is the sloppy-top / strict-function shape the spec asks
// for.
const frozen = Object.freeze({ a: 1 });

function strictOuter(): void {
  "use strict";
  const block = () => {
    frozen.a = 2;
  };
  const expr = () => (frozen.a = 3);
  const nested = () => () => {
    frozen.a = 4;
  };
  const deep = function () {
    return () => {
      frozen.a = 5;
    };
  };
  for (const [label, f] of [
    ["block", block],
    ["expr", expr],
    ["nested", nested()],
    ["deep", deep()],
  ] as any) {
    try {
      f();
      console.log(label, "no throw");
    } catch (e) {
      console.log(label, (e as any).constructor.name);
    }
  }
  // A nested `function` keeps answering for itself, unchanged.
  const fnex = function () {
    frozen.a = 6;
  };
  try {
    fnex();
    console.log("fnex", "no throw");
  } catch (e) {
    console.log("fnex", (e as any).constructor.name);
  }
}
strictOuter();

// Sloppy enclosing code: the refusal is discarded and the assignment
// still answers its right-hand side.
const sloppyArrow = () => {
  frozen.a = 7;
  return frozen.a;
};
console.log("sloppy", sloppyArrow());

// An arrow's own directive still works when nothing is inherited.
const ownDirective = () => {
  "use strict";
  frozen.a = 8;
};
try {
  ownDirective();
  console.log("own", "no throw");
} catch (e) {
  console.log("own", (e as any).constructor.name);
}
