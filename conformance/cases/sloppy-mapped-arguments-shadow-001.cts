// A block binding named like a parameter shadows it only inside its
// block: a parameter write outside still shows in `arguments[0]`, and
// `arguments[0]` read outside the block names the parameter.
function blockLet(a) { a = 2; { let a = 5; a++; } return arguments[0] + "," + a; }
var blockLetV = blockLet;
console.log("blockLet", blockLet(1), blockLetV(1));
function loopLet(a) { a = 3; for (let i = 0; i < 1; i++) { let a = 9; } return arguments[0]; }
var loopLetV = loopLet;
console.log("loopLet", loopLet(1), loopLetV(1));
function redeclared(a) { var a = 5; return arguments[0] + "," + a; }
var redeclaredV = redeclared;
console.log("redeclared", redeclared(1), redeclaredV(1));
function forOf(a) { var out = ""; for (a of [4]) { out += arguments[0]; } return out + "," + a; }
var forOfV = forOf;
console.log("forOf", forOf(1), forOfV(1));
function inner(a) { { let a = 5; arguments[0] = 7; } return arguments[0]; }
var innerV = inner;
console.log("inner", inner(1), innerV(1));
