// §14.12.1 — both the switch scrutinee and a case value are spelled
// `Expression`, which is the comma-separated production (§13.16), not
// AssignmentExpression. tr parsed both with the assignment-level
// entry and rejected the comma:
//
//   switch (2, 2) { … }   -> expected `)` after switch scrutinee
//   case 1, 2:            -> expected `:` after case value
//
// The `for` step clause is the third place the grammar says
// `Expression`, and it had grown its own copy of the loop. All three
// share one spelling now.

// Scrutinee: the list evaluates left to right, the LAST one is the
// value being switched on.
let order: string[] = [];
switch ((order.push("a"), 2)) {
  case 2:
    console.log("scrutinee list ->", order.join(","));
}

// Case value: same, and the earlier clauses' side effects happen in
// clause order until one matches.
let seen = 0;
switch (2) {
  case (seen += 1, 3):
    console.log("not this one");
    break;
  case (seen += 1, 2):
    console.log("case list ->", seen);
}

// The bare (unparenthesised) form is what test262's switch scope
// probes are written in.
switch (2) {
  case 1, 2:
    console.log("bare case list");
}

// The `for` step keeps working through the shared spelling.
for (let i = 0, j = 5; i < 2; i++, j--) {
  console.log(i, j);
}

// A plain single-expression switch is untouched.
switch ("k") {
  case "k":
    console.log("plain");
    break;
  default:
    console.log("default");
}
