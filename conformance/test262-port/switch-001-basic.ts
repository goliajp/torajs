// Adapted from test262: language/statements/switch/* — strict-eq
// dispatch with default and break.
function classify(x: number): string {
  switch (x) {
    case 0: return "zero";
    case 1: return "one";
    case 2: return "two";
    default: return "other";
  }
}

function check(): number {
  if (classify(0) !== "zero") { throw "#1"; }
  if (classify(1) !== "one") { throw "#2"; }
  if (classify(2) !== "two") { throw "#3"; }
  if (classify(99) !== "other") { throw "#4"; }
  return 0;
}
console.log(check());
