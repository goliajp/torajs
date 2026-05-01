// Adapted from test262: built-ins/String/prototype/repeat/* —
// `s.repeat(n)` produces a fresh String containing s n times. Single
// runtime alloc + n memcpy's. Negative n is clamped to 0 (avoids the
// JS spec's RangeError; the test set doesn't exercise that path).
function check(): number {
  if ("ab".repeat(3) !== "ababab") { throw "#1"; }
  if ("x".repeat(0) !== "") { throw "#2"; }
  if ("hi".repeat(1) !== "hi") { throw "#3"; }
  if ("a".repeat(5) !== "aaaaa") { throw "#4"; }
  if ("".repeat(7) !== "") { throw "#5"; }

  // Use in template strings.
  let dashes = "-".repeat(4);
  if (`<${dashes}>` !== "<---->") { throw "#6"; }
  return 0;
}
console.log(check());
