// Adapted from test262: built-ins/String/prototype/substring/* — slice
// is JS's modern half. substring predates it (ES1) and diverges on
// negatives (clamped to 0 instead of wrapping) and on swapped indices
// (silently swapped). tr's runtime helper applies both fix-ups before
// memcpy'ing the byte range.
function check(): number {
  // Basic in-bounds.
  if ("hello".substring(0, 5) !== "hello") { throw "#1: full"; }
  if ("hello".substring(0, 3) !== "hel") { throw "#2: prefix"; }
  if ("hello".substring(2, 5) !== "llo") { throw "#3: suffix"; }
  if ("hello".substring(1, 4) !== "ell") { throw "#4: middle"; }
  if ("hello".substring(2, 2) !== "") { throw "#5: empty range"; }

  // Negatives clamp to 0 — substring's signature divergence from slice.
  if ("hello".substring(-3, 4) !== "hell") { throw "#6: neg start"; }
  if ("hello".substring(0, -2) !== "") { throw "#7: neg end clamps to 0"; }
  if ("hello".substring(-5, -1) !== "") { throw "#8: both neg"; }

  // start > end — silently swapped.
  if ("hello".substring(4, 1) !== "ell") { throw "#9: swap"; }
  if ("hello".substring(5, 0) !== "hello") { throw "#10: swap full"; }

  // Out-of-bounds clamp to len.
  if ("hello".substring(0, 100) !== "hello") { throw "#11: end OOB"; }
  if ("hello".substring(100, 0) !== "hello") { throw "#12: swap+OOB"; }

  // Empty receiver.
  if ("".substring(0, 5) !== "") { throw "#13: empty recv"; }

  // Identity vs slice on the simple positive cases.
  let s = "abcdefgh";
  if (s.substring(2, 5) !== s.slice(2, 5)) { throw "#14: equiv slice"; }

  return 0;
}
console.log(check());
