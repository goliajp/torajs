// Adapted from test262: language/statements/if/*.js — number-only branches.
function check(): number {
  let r: number = 0;
  if (true) { r = 1; } else { r = 2; }
  if (r !== 1) { throw "#1"; }
  if (false) { r = 3; } else { r = 4; }
  if (r !== 4) { throw "#2"; }
  if (1 < 2) { r = 5; } else { r = 6; }
  if (r !== 5) { throw "#3"; }
  return 0;
}
console.log(check());
