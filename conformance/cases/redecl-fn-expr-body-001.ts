// §14.2.1 — a Block's lexical names may not recur in its
// VarDeclaredNames, and that holds inside a function or arrow
// EXPRESSION body as much as inside a declaration's. In strict code
// two function declarations in one block are duplicate lexical names
// too: the Annex B allowance is for sloppy code only. These are
// parse-phase errors of the whole program, so the rejected shapes are
// listed here and were checked by hand against bun:
//   (function () { { let f; var f; } })
//   (() => { { const f = 1; var f; } })
//   "use strict"; { function f() {} function f() {} }
// The legal neighbours below compile and run.
const a = function () {
  {
    let f = 1;
  }
  var f = 2;
  return f;
};
const b = () => {
  {
    const g = 3;
  }
  {
    var g = 4;
  }
  return g;
};
const c = function () {
  let h = 5;
  {
    let h = 6;
  }
  return h;
};
console.log(a(), b(), c());
