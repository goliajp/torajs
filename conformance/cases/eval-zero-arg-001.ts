// eval() with no argument is eval(undefined): a non-String argument
// comes back unchanged (§19.2.1.1 step 2), in either call form.
var a = eval();
var b = (0, eval)();
console.log(a, b, typeof a);
eval();
console.log("after");
