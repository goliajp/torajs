// rotation 638 — §14.7.5: in sloppy code `for (let in obj)` takes `let` as the
// identifier that is the head's LeftHandSideExpression (the `[lookahead ≠ let]`
// restriction only excludes `let` before `of` and `let [`).
// oracle: node script goal.
var obj = { key: 1 };
var let;
for (let in obj) ;
console.log("lhs", let);
for (let in {}) { }
console.log("done", let);
