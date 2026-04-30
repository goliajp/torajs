// Adapted from test262: language/statements/throw + try/catch with
// non-number throw values — string is the most common case.
function check(): number {
  let caught: string = "";
  try {
    throw "boom";
  } catch (e: string) {
    caught = e;
  }
  if (caught !== "boom") { throw "#1"; }
  return 0;
}
console.log(check());
