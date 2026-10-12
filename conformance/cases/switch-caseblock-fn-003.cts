// Function declarations in a switch CaseBlock, reached through the
// other ways a CaseBlock occurs: a labeled switch, a switch nested in
// a clause that declares the same name again (the inner CaseBlock's
// binding shadows the outer one only inside it), arrow and function
// expression bodies, a loop body, and a case selector that calls a
// function declared in a later clause (§14.12.4 evaluates selectors
// in the CaseBlock's environment).
//
// Oracle is node (`node <file>.cts`); bun answers ReferenceError for
// the cross-clause calls, which the spec does not allow.

function lab(x: number): string {
  let out = "";
  lbl: switch (x) {
    case 0:
      function f() { return "f" }
      break lbl;
    case 1:
      out = f();
      break lbl;
  }
  return out;
}
console.log(lab(1), lab(0));
function nest(x: number, y: number): string {
  switch (x) {
    case 0:
      function g() { return "outer" }
      return "0";
    case 1:
      switch (y) {
        case 0:
          function g() { return "inner" }
          return "x";
        case 1:
          return g();
      }
      return g();
  }
  return "none";
}
console.log(nest(1, 1), nest(1, 2));
const arrow = (x: number): number => {
  switch (x) {
    case 0:
      function k() { return 9 }
      break;
    case 1:
      return k();
  }
  return 0;
};
console.log(arrow(1));
const fe = function (x: number): number {
  switch (x) {
    case 0:
      function m() { return 11 }
      break;
    case 1:
      return m() + m();
  }
  return 0;
};
console.log(fe(1));
for (let i = 0; i < 2; i++) {
  switch (i) {
    case 0:
      continue;
    case 1:
      function lp() { return "loop" }
      console.log(lp());
  }
}
function selectorUse(x: number): string {
  switch (x) {
    case sel():
      return "matched";
    case 9:
      function sel() { return 1 }
  }
  return "no";
}
console.log(selectorUse(1));
