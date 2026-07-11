// Annex B B.2.2 String.prototype HTML methods — CreateHTML wraps.
// Attributed forms escape `"` as &quot; in the attribute value and
// run ToString on it unconditionally (numbers, undefined legal).

console.log("x".anchor("n"));
console.log("x".link("http://a/b?q=1"));
console.log("x".fontcolor("red"), "x".fontsize(7));
console.log("q".anchor("a\"b"));
console.log("x".big(), "x".blink(), "x".bold(), "x".fixed());
console.log("x".italics(), "x".small(), "x".strike(), "x".sub(), "x".sup());
// UTF-16 receiver / attribute value widen the result
console.log("汉".bold(), "x".anchor("宽"));
// empty receiver / empty attribute
console.log("".big(), "x".anchor(""));
// missing / undefined attribute renders "undefined"
console.log("x".anchor(undefined as any));
// Substr view receiver materializes
console.log(("ab" + "cd").slice(1, 3).bold());
// chaining
console.log("x".bold().italics().big());
