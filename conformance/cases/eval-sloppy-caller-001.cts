// The negative half of §19.2.1.1 step 8: nothing here is strict code,
// so a direct eval inherits nothing and its text keeps every sloppy
// admission. This file is `.cts` — the sloppy script goal — and it has
// no prologue, no class, and no enclosing strict function.
//
// Each check reads the eval's own completion value rather than a name
// it declared: whether a sloppy direct eval's `var` escapes into the
// caller is a separate question about scope, and this file is about
// what the text is allowed to SAY.

// §12.7.2 — `yield` and the future reserved words are ordinary names.
console.log("yield", eval("(function (yield) { return yield; })(7)"));
console.log("reserved", eval("(function (statiC) { return statiC; })(8)"));
// annexB §B.1.1 — legacy octal keeps the value the lexer gave it.
console.log("octal", eval("010"));
// annexB §B.3.2 / §B.3.4 — the two positions those productions hand a
// FunctionDeclaration back to are exactly what the extension is for.
eval("if (true) function ib(){ return 'if'; }");
eval("l1: function lb(){ return 'label'; }");
console.log("annexb", ib(), lb());
// §15.1.2 — a simple parameter list in sloppy code may repeat a name,
// and the later binding wins.
console.log("dup-param", eval("(function (a, a){ return a; })(1, 2)"));
// §14.11 — `with` is an ordinary statement in sloppy code.
eval("var o = { hidden: 3 }; with (o) { console.log('with', hidden); }");
