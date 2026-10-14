// More places a touch of `arguments` can hide from the mapped faces:
// a conditional, a template substitution, a `new` argument list, a
// finally block, and calls that pass fewer arguments than parameters.
// Each function is called directly and through a value.
function partialWrite(a, b) { arguments[1] = 5; return a + "," + b + "," + arguments[1] + "," + arguments.length; }
var partialWriteV = partialWrite;
console.log("partialWrite", partialWrite(1), partialWriteV(1), partialWriteV(1, 2));
function partialParam(a, b) { b = 6; return arguments[1] + "," + b; }
var partialParamV = partialParam;
console.log("partialParam", partialParam(1), partialParamV(1), partialParamV(1, 2));
function cond(a) { var t = a ? delete arguments[0] : 0; return a + "," + arguments[0]; }
var condV = cond;
console.log("cond", cond(1), condV(1));
function tmpl(a) { var s = `${(delete arguments[0], 1)}`; return a + "," + arguments[0] + "," + s; }
var tmplV = tmpl;
console.log("tmpl", tmpl(1), tmplV(1));
function Holder(x) { this.a = x; }
function newArgs(a) { var x = new Holder((delete arguments[0], 1)); return a + "," + arguments[0] + "," + x.a; }
var newArgsV = newArgs;
console.log("newArgs", newArgs(1), newArgsV(1));
function fin(a) { try { throw 0; } catch (e) { } finally { delete arguments[0]; } return a + "," + arguments[0]; }
var finV = fin;
console.log("fin", fin(1), finV(1));
function spread(a) { var x = [...arguments]; x[0] = 3; return a + "," + arguments[0]; }
var spreadV = spread;
console.log("spread", spread(1), spreadV(1));
function triple(a, a, a) { a = 9; return arguments[0] + "," + arguments[1] + "," + arguments[2]; }
var tripleV = triple;
console.log("triple", triple(1, 2, 3), tripleV(1, 2, 3), triple(1, 2), tripleV(1, 2));
