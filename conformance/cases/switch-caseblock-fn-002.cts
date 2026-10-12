"use strict";
// The strict twin of switch-caseblock-fn-001: the CaseBlock binding is
// the same, and Annex B §B.3.3 adds no var binding, so `typeof v`
// outside the switch is "undefined" whichever clause ran. Two
// same-named declarations in one CaseBlock are an early error here
// (§14.12.1), so the twin leaves that case out.
//
// Oracle is node (`node <file>.cts`); bun answers ReferenceError for
// the cross-clause call, which the spec does not allow.

function sw(x: number): number {
  switch (x) {
    case 0:
      function f() { return 7 }
      return 0;
    case 1:
      return f();
  }
  return -1;
}
console.log(sw(1), sw(0));

// declared in `default`, called from an earlier case
function before(x: number): number {
  switch (x) {
    case 1:
      return h();
    default:
      function h() { return 42 }
      return 0;
  }
}
console.log(before(1), before(5));

// the var binding is written only when the declaration runs
function varWrite(x: number): string {
  switch (x) {
    case 0:
      function v() { return "v" }
      break;
    case 1:
      break;
  }
  return typeof v;
}
console.log(varWrite(0), varWrite(1));

// the scrutinee is evaluated outside the CaseBlock, so it still sees
// the outer `s`
function scrut(): string {
  var s: any = "outer";
  switch (typeof s) {
    case "string":
      function s() { return "inner" }
      return typeof s + "/" + s();
  }
  return "none";
}
console.log(scrut());

// a let in another clause is still in its dead zone
function tdz(x: number): string {
  try {
    switch (x) {
      case 0:
        let a = 1;
        return "a" + a;
      case 1:
        return "b" + a;
    }
  } catch (e) {
    return (e as Error).constructor.name;
  }
  return "none";
}
console.log(tdz(0), tdz(1));

// a nested function in another clause calls it too
function nested(x: number): number {
  switch (x) {
    case 0:
      function base() { return 5 }
      return 0;
    case 1:
      function twice() { return base() * 2 }
      return twice();
  }
  return -1;
}
console.log(nested(1));

async function af(x: number) {
  switch (x) {
    case 0:
      function r() { return 4 }
      return 0;
    case 1:
      return r();
  }
  return -1;
}

function* gen(x: number) {
  let out = 0;
  switch (x) {
    case 0:
      function q() { return 3 }
      break;
    case 1:
      out = q();
  }
  yield out;
}
console.log([...gen(1)].join(","));

switch (2) {
  case 1:
    function top1() { return "t1" }
    break;
  case 2:
    console.log(top1(), typeof top1);
}

af(1).then((v) => console.log("async", v));
