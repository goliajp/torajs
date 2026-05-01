// Adapted from test262: built-ins/Math/random/* — Math.random returns
// a uniform double in [0, 1). tr's runtime uses libc rand()/RAND_MAX
// scaled. JS spec is implementation-defined for the simple use case;
// the only invariants are the range and that successive calls don't
// crash.
function check(): number {
  for (let i: number = 0; i < 100; i = i + 1) {
    let r = Math.random();
    if (r < 0) { throw "#1: below 0"; }
    if (r >= 1) { throw "#2: at or above 1"; }
  }
  return 0;
}
console.log(check());
