"use strict";
// §14.12.4: a function declared in a switch clause is the CaseBlock's
// binding everywhere in its clauses except below a scope that binds the
// same name again. A nested `let`, loop head, `catch` parameter or
// arrow parameter hides it only inside that scope; the condition, the
// other branch and the statements around it still read the CaseBlock's
// function, never a binding outside the switch.
//
// Oracle is node (`node <file>.cts`).

function cond(): number {
  const f = () => 1;
  switch (0) {
    case 0:
      function f() { return 7 }
      if (f() === 7) { let f = 0; return f; }
      return -1;
  }
  return -2;
}
console.log(cond());

function elseBranch(x: number): number {
  const f = () => 1;
  switch (x) {
    case 0:
      function f() { return 7 }
      if (x > 0) { let f = 3; return f; } else { return f(); }
  }
  return -2;
}
console.log(elseBranch(0), elseBranch(1));

function loopHead(): number {
  const f = () => 1;
  let sum = 0;
  switch (0) {
    case 0:
      function f() { return 7 }
      for (let f = 0; f < 2; f++) { sum += f; }
      sum += f();
  }
  return sum;
}
console.log(loopHead());

function catchParam(): string {
  const f = () => "outer";
  switch (1) {
    case 0:
      function f() { return "case" }
    case 1:
      try { throw "thrown"; } catch (f) { return f + " " + typeof f; }
  }
  return "none";
}
console.log(catchParam());

function catchAround(): string {
  const f = () => "outer";
  switch (1) {
    case 0:
      function f() { return "case" }
    case 1:
      let r = "";
      try { throw "x"; } catch (f) { r = String(f); }
      return r + " " + f();
  }
  return "none";
}
console.log(catchAround());

function arrowParam(): number {
  const f = () => 1;
  switch (0) {
    case 0:
      function f() { return 7 }
      const h = (f: number) => f * 10;
      const k = () => f();
      return h(2) + k();
  }
  return -2;
}
console.log(arrowParam());

// capturing: the router moves it in front of the switch under a slot
function capturing(n: number): number {
  const f = () => 1;
  switch (0) {
    case 0:
      function f() { return n }
      if (f() === n) { let f = 100; return f + n; }
      return -1;
  }
  return -2;
}
console.log(capturing(5));
