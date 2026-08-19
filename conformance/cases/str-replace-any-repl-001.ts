// §22.1.3.19 step 5 — functionalReplace = IsCallable(replaceValue) is
// a RUNTIME question when the replaceValue arrives as `any`: a
// callable cell invokes per match, everything else is step 12's
// ToString. The static lanes used to splice a function's source text
// into the output (the fn-return face's promoted values are exactly
// any-typed).
function getFn() { return function () { return "c"; }; }
var seen: any = "sentinel";
function getFn2() { return function () { seen = this; return "C"; }; }
console.log("ab".replace("b", getFn()));
console.log("ab".replace("b", getFn2()), seen === undefined);
var r: any = "str-repl";
console.log("ab".replaceAll("b", r));
var n: any = 42;
console.log("ab".replace("b", n));
