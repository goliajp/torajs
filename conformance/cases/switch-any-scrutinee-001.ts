// §14.12.4 — CaseClauseIsSelected compares with IsStrictlyEqual, which
// is total: it answers false across types rather than being undefined
// for them. So an `any` scrutinee admits any case value, and the
// compare has to be the same runtime one `===` performs — a raw
// integer compare of a boxed word against a bare one is never equal,
// which would take every switch to its default.

const n: any = 15;
switch (n) {
  case 14:
    console.log("fourteen");
    break;
  case 15:
    console.log("fifteen");
    break;
  default:
    console.log("none");
}

// A case the scrutinee cannot equal still falls through correctly.
const s: any = "b";
switch (s) {
  case "a":
    console.log("a");
    break;
  case "b":
    console.log("b");
    break;
  default:
    console.log("no string");
}

// Types that differ answer false rather than matching — the default
// is the right arm, not an accident.
const t: any = "15";
switch (t) {
  case 15:
    console.log("number 15 matched a string");
    break;
  default:
    console.log("no cross-type match");
}

// An `any` case value against a concrete scrutinee is the same
// question from the other side.
const fifteen: any = 15;
const m: number = 15;
switch (m) {
  case fifteen:
    console.log("concrete matched any");
    break;
  default:
    console.log("missed");
}
