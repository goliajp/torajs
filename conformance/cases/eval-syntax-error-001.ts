// A literal eval source that does not parse raises SyntaxError when the
// eval is EVALUATED (§19.2.1.1 step 12) — not when the program is
// compiled. An unreachable eval of garbage is a valid program.

if (false) {
  eval("this is not valid (((");
}
console.log("unreachable eval raised nothing");

try {
  eval("((( not valid");
  console.log("NOT REACHED");
} catch (e) {
  console.log("caught SyntaxError:", e instanceof SyntaxError);
}

// the throw happens at the right point in the sequence
console.log("before");
try {
  eval("var = = =");
} catch (e) {
  console.log("caught second:", e instanceof SyntaxError);
}
console.log("after");

// a valid eval next to a broken one still runs
eval("console.log('valid eval still runs');");

// inside a closure
const f = () => {
  try {
    eval("}{");
  } catch (e) {
    return e instanceof SyntaxError;
  }
  return false;
};
console.log("in closure:", f());
