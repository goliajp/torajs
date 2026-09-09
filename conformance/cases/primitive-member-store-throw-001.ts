// A member store raises whatever the receiver is (§10.1.8.1 — a
// strict-mode write to a property of a primitive is a TypeError).
// The caller's throw check must not be pruned away: a specialized
// body types its parameter as the ARGUMENT's type, so `n` here is
// Number rather than Any, and the may-throw whitelist that only
// named Any / arr-length / Struct receivers left this store out.
// The callee stopped, but the caller printed its ret sentinel:
// `console.log(f(1))` answered `null` and this try/catch caught
// nothing. The same program spelled `n: any` threw all along.
function f(n) {
  n.nope = 1;
  console.log("NOT REACHED");
  return "after";
}
try {
  console.log(f(1));
} catch (e) {
  console.log("caught", (e as Error).constructor.name);
}
try {
  console.log(f("s"));
} catch (e) {
  console.log("caught", (e as Error).constructor.name);
}
try {
  console.log(f(true));
} catch (e) {
  console.log("caught", (e as Error).constructor.name);
}
