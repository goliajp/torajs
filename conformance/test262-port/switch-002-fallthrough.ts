// Adapted from test262: switch fall-through — when a case body has no
// `break`, control falls through to the next case's body.
function pricing(tier: number): number {
  let p: number = 0;
  switch (tier) {
    case 3: p += 100;       // tier 3 gets full
    case 2: p += 50;        // tier 2 + 3 get this
    case 1: p += 25;        // tier 1 + 2 + 3 get this
      break;
    default: p = 0;
  }
  return p;
}

function check(): number {
  if (pricing(1) !== 25) { throw "#1"; }
  if (pricing(2) !== 75) { throw "#2"; }
  if (pricing(3) !== 175) { throw "#3"; }
  if (pricing(99) !== 0) { throw "#4"; }
  return 0;
}
console.log(check());
