// A generator EXPRESSION bound to a let and passed to a fn-typed
// parameter (the test262 eval-code harness shape): the hoisted
// factory's binding re-reprs Closure, so the annotated param must
// retag off the bare-pointer lane.
function check(fn: () => void): void {
  try {
    fn();
    console.log("returned");
  } catch (e) {
    console.log("threw");
  }
}
let f = function* () {
  yield 7;
};
check(f);
