// §19.2.1.1 — eval code's strictness is its OWN, and that has two
// halves. One is the CALL: a direct eval inherits the caller, an
// indirect one never does (its code is global sloppy code whatever the
// caller was). The other is the TEXT: a source opening with
// `"use strict"` is strict code whichever call reached it.
//
// Reading only the call half gives a block-nested declaration in a
// strict eval text the Annex B §B.3.3 var binding it must not have, so
// the declaration escapes its block and the ReferenceError the program
// is written to catch never arrives.

// Strict text through an indirect call: the declaration stays in its
// block, so the name does not resolve afterwards. `typeof` asks
// without throwing.
(0, eval)('"use strict";{ function f() {} }');
console.log("strict-indirect", typeof f);

// Same text, same call, reached through a switch case — the other
// spelling test262 checks.
(0, eval)('"use strict";switch (1) { case 1: function g() {} }');
console.log("strict-switch", typeof g);

// Sloppy text through the same indirect call still gets its Annex B
// answer — that is what the previous cut fixed and this must not undo.
(0, eval)("{ function h() {} }");
console.log("sloppy-indirect", typeof h);

(0, eval)("switch (1) { case 1: function i2() {} }");
console.log("sloppy-switch", typeof i2);

// A prologue deeper in the text is not a prologue.
(0, eval)('{ "use strict"; function j() {} }');
console.log("not-a-prologue", typeof j);
