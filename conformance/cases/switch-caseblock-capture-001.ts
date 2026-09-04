// §14.12.4 — a closure in one clause captures a binding another clause
// declares, because the CaseBlock is ONE environment. The clause
// bodies are sibling basic blocks, so a capture box minted where the
// declaration sits is defined in a block that dominates none of the
// others; the boxes are minted before the compare chain instead.
//
// Not covered here, and recorded rather than fixed: the same reach
// when the declaring clause does NOT run
// (`switch (1) { case 0: let a; case 1: … a … }`). That one used to
// abort at regalloc — the constant scrutinee deletes the declaring
// block outright — and now compiles, but the spec answer is a TDZ
// ReferenceError and tr has no TDZ, so it reads `undefined`. There is
// no oracle-matching fixture for it until TDZ lands: every shape that
// reproduces the crash is a shape the spec wants to throw on.

// Declared under a matched clause, captured from the clause it falls
// through into.
switch (0) {
  case 0:
    let b = "value";
  case 1:
    console.log(
      (function () {
        return b;
      })(),
    );
}

// One binding, written from a closure in the declaring clause and read
// from a closure in the next: both see the same cell.
let out: string[] = [];
switch (0) {
  case 0:
    let c = 1;
    out.push(
      String(
        (function () {
          c = c + 1;
          return c;
        })(),
      ),
    );
  case 1:
    out.push(
      String(
        (function () {
          return c;
        })(),
      ),
    );
}
console.log(out.join(","));

// A capture that stays inside the clause declaring it keeps working.
switch (0) {
  case 0: {
    let d = 7;
    console.log(
      (function () {
        return d;
      })(),
    );
    break;
  }
}

// And one under `default`.
switch (9) {
  case 0:
    console.log("no");
    break;
  default:
    let e = "dflt";
    console.log(
      (function () {
        return e;
      })(),
    );
}
