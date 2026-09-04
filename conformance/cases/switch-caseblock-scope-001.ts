// §14.12.4 — the CaseBlock is ONE declarative environment covering
// every clause and the default, not one per clause.
//
// Three consequences, all of which tr had wrong:
//
// 1. A clause's lexical declaration does not escape the switch.
// 2. It does not permanently replace an outer binding of the same
//    name — the outer one is back after the switch.
// 3. A name declared in one clause is visible in the next: the
//    clauses share the environment, they do not each get one.

// 1 — no escape, from a matched clause and from an unmatched one.
switch (0) {
  case 0:
    let a = 1;
    console.log(a);
}
console.log(typeof a);

switch (2) {
  case 0:
    let unreached = "u" + "x";
    break;
  default:
    console.log("default");
}
console.log(typeof unreached);

// 2 — shadowing is restored, for a Copy binding and a refcounted one.
let n = 9;
let s = "out";
switch (0) {
  default:
    let n = 1;
    let s = "in" + "";
    console.log(n, s);
}
console.log(n, s);

// 3 — one environment across clauses: the binding declared under
// `case 0` is the same binding the fall-through clause reads.
switch (0) {
  case 0:
    let shared = "sh" + "ared";
  case 1:
    console.log(shared);
}
console.log(typeof shared);

// A block nested in a clause still gets its own frame inside the
// CaseBlock's, and `break` out of the clause releases both.
switch (0) {
  case 0: {
    let inner = "in" + "ner";
    console.log(inner);
    break;
  }
}
console.log(typeof inner);
