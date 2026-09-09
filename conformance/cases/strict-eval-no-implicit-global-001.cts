// §19.2.1.1 gives eval code its own strictness, and a `"use strict"`
// directive prologue in the TEXT is one of the two ways it gets it —
// the other being a direct eval inheriting the caller. Strict code
// does not create globals: an assignment to an unresolvable name
// throws a ReferenceError instead.
//
// tr splices the parsed eval text into the program, and by the time
// the sloppy-implicit-globals pass runs those statements look like the
// rest of a sloppy script — same shape, and the spans were blanked at
// the splice. So the pass synthesized `var` for a name only strict
// code ever assigned, and the ReferenceError never came.

function attempt(fn: any) {
  try {
    fn();
    return "no throw";
  } catch (e) {
    return (e as any).name;
  }
}

// Strict text: the write throws, and nothing is created.
console.log(attempt(function () {
  eval('"use strict"; strictEvalTarget = 1;');
}));
console.log("after-strict", typeof strictEvalTarget);

// Sloppy text right next to it: the write creates the global, which is
// what makes the line above evidence of anything.
console.log(attempt(function () {
  eval("sloppyEvalTarget = 2;");
}));
console.log("after-sloppy", typeof sloppyEvalTarget, sloppyEvalTarget);

// And an ordinary sloppy assignment outside any eval still creates.
plainTarget = 3;
console.log("plain", typeof plainTarget, plainTarget);
