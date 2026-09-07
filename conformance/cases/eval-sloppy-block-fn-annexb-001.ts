// §19.2.1.1 — eval code's strictness is its OWN. An INDIRECT eval's
// code is global sloppy code whatever the calling program was, so
// Annex B §B.3.3 gives a block-nested `function` there a var-scoped
// binding, even though this file is a module and strict throughout.
//
// The splice blanks every spliced declaration's span (it indexes the
// eval text, a different string), and every synthesized declaration
// carries that same `(0, 0)` sentinel — so the span cannot answer the
// question and the program's goal answers it for the wrong program.
// All three spellings of a block-nested declaration must agree.
(0, eval)("if (true) function bare(){ return 'bare'; }");
console.log(typeof (0, eval)("bare"));

(0, eval)("if (true) { function braced(){ return 'braced'; } }");
console.log(typeof (0, eval)("braced"));

(0, eval)("{ function blk(){ return 'blk'; } }");
console.log(typeof (0, eval)("blk"));

// The module's OWN block declaration is strict code and keeps the
// block binding alone — the channel must not leak the other way.
{ function mine() { return "mine"; } }
console.log(typeof mine);
