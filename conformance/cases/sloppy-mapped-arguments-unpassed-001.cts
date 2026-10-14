// A position the caller did not pass is not mapped: writing
// `arguments[k]` leaves parameter k alone, and writing the parameter
// leaves `arguments[k]` alone. Every call here is direct.
function writeSlot(a, b) { arguments[1] = 5; return a + "," + b + "," + arguments[1] + "," + arguments.length; }
console.log("writeSlot", writeSlot(1));
function writeParam(a, b) { b = 6; return arguments[1] + "," + b; }
console.log("writeParam", writeParam(1));
function passed(a, b) { a = 3; return arguments[0] + "," + arguments.length; }
console.log("passed", passed(1));
