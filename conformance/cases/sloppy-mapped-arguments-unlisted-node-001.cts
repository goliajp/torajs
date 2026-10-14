// A touch of `arguments` inside a labeled statement, a comma
// expression, an await or a `new` argument list still decides whether
// the parameters can stand in for the arguments object.
function labeled(a) { L: { delete arguments[0]; } return a + "," + arguments[0]; }
var labeledV = labeled;
console.log("labeled", labeled(1), labeledV(1));
function comma(a) { (0, Object.defineProperty(arguments, "0", { value: 2 })); return arguments[0]; }
var commaV = comma;
console.log("comma", comma(1), commaV(1));
function deleteInComma(a) { var s = (delete arguments[0], a + "," + arguments[0]); return s; }
var deleteInCommaV = deleteInComma;
console.log("deleteInComma", deleteInComma(1), deleteInCommaV(1));
function Box(x) { this.x = x; }
function newArg(a) { var b = new Box(arguments); b.x[0] = 9; return arguments[0]; }
var newArgV = newArg;
console.log("newArg", newArg(1), newArgV(1));
function callThis(a, g) { var r = arguments[1](); return r[0] + "," + r.length; }
var callThisV = callThis;
function self() { return this; }
console.log("callThis", callThis(1, self), callThisV(1, self));
async function awaited(a) { await Object.defineProperty(arguments, "0", { value: 3 }); return arguments[0]; }
var awaitedV = awaited;
awaited(1).then(function (v) { console.log("awaited", v); });
awaitedV(1).then(function (v) { console.log("awaitedV", v); });
