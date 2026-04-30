// Adapted from test262: language/statements/try/*.js — number throws.
// Drops cases that test specific error class types (TypeError, RangeError) —
// torajs throws are raw values, not Error instances.
function check(): number {
  let r: number = 0;
  try {
    throw 42;
  } catch (e) {
    r = e;
  }
  if (r !== 42) { throw "#1: catch binds throw value"; }

  try {
    try {
      throw 1;
    } catch (e) {
      throw e + 10;
    }
  } catch (e) {
    r = e;
  }
  if (r !== 11) { throw "#2: re-throw with computed value"; }

  return 0;
}
console.log(check());
