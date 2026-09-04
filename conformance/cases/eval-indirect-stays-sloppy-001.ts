// §19.2.1.1 step 3 — an INDIRECT eval never inherits the caller. Its
// code is global sloppy code whatever the calling code was, so in this
// module — strict code throughout — every form the direct lane refuses
// still runs here.
(0, eval)("var yield = 1");
console.log("yield", (0, eval)("yield"));
(0, eval)("var oct = 010");
console.log("octal", (0, eval)("oct"));
(0, eval)("if (true) function ib(){}");
console.log("annexb", (0, eval)("typeof ib"));
// §15.1.2's shape, spelled as a function EXPRESSION: a declaration out
// of an indirect eval would have to land on the global object, which is
// a separate surface from this file's question.
(0, eval)("var idup = function (a, a){ return a; };");
console.log("dup-param", (0, eval)("idup(1, 2)"));
