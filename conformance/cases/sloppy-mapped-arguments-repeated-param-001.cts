// A repeated parameter name binds its last position, and only that
// position is mapped; the earlier one keeps its own value.
function pair(a, a) { return arguments[0] + "," + arguments[1] + "," + a; }
var pairV = pair;
console.log("pair", pair(1, 2), pairV(1, 2));
function writeFirst(a, a) { arguments[0] = 7; return a + "," + arguments[0]; }
var writeFirstV = writeFirst;
console.log("writeFirst", writeFirst(1, 2), writeFirstV(1, 2));
function writeLast(a, a) { arguments[1] = 8; return a + "," + arguments[0]; }
var writeLastV = writeLast;
console.log("writeLast", writeLast(1, 2), writeLastV(1, 2));
function writeParam(a, b, a) { a = 5; return arguments[0] + "," + arguments[2] + "," + b; }
var writeParamV = writeParam;
console.log("writeParam", writeParam(1, 2, 3), writeParamV(1, 2, 3));
console.log("short", writeParam(1), writeParamV(1));
