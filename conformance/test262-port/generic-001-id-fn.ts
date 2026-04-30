// Adapted from TS generic function — exercises monomorphization at three
// concrete call sites. tr uses call-site inference (no explicit `<T>`
// args at the call), so the type argument is recovered from the value.
function identity<T>(x: T): T { return x; }

function check(): number {
  if (identity(42) !== 42) { throw "#1"; }
  if (identity(-7) !== -7) { throw "#2"; }
  if (identity(true) !== true) { throw "#3"; }
  return 0;
}
console.log(check());
