// §13.16 Expression is the comma list, and every statement head that
// spells `Expression` takes it: return, throw, if, while, do-while,
// the for condition, with (sloppy only, not spelled here). The value
// is the LAST operand; earlier ones run for effect.
var trace: string[] = [];
function t(s: string, v: any) { trace.push(s); return v; }
function f(n: number): any { if (n === 0) { return 7; } return t("r", 0), f(n - 1); }
console.log(f(3), trace.join(""));
function g(): number { try { throw t("a", 1), t("b", new Error("x")); } catch (e) { return (e as Error).message.length; } }
console.log(g(), trace.join(""));
var i = 0;
if (t("i1", 0), t("i2", true)) { i = 1; }
while (t("w", i), i < 3) { i++; }
do { i++; } while (t("d", 0), i < 5);
for (var j = 0; t("f", 0), j < 2; j++) { i++; }
console.log(i, trace.join(""));
