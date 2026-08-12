// §16.1 — module code is strict, so a function called with no
// receiver gets `undefined` rather than the global object
// (§10.2.1.2 step 5 instead of step 6). This file is a module, and
// nothing in it says `"use strict"`: the goal alone has to arm it.
// The `.cts` sibling of this shape keeps answering `object`, which is
// what makes this pair worth pinning rather than either half alone.
function detached() {
  return typeof this;
}
console.log(detached());

const alias = detached;
console.log(alias());

function outer() {
  function inner() {
    return typeof this;
  }
  return inner();
}
console.log(outer());

// Methods still receive their receiver — strictness changes only what
// happens when there is none.
const o = {
  n: 7,
  read() {
    return this.n;
  },
};
console.log(o.read());

// (A method DETACHED from its object and then called belongs here too
// — it should throw, since the read is `undefined.n`. It cannot be
// pinned yet: `const t = o.read; t()` faults, and it faults the same
// way in a sloppy `.cts`, so that hole predates the goal seed and is
// tracked on its own.)
