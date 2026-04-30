// Adapted from test262: language/statements/if/* — deep if/else if chain.
function classify(n: number): string {
  if (n < 0) {
    return "negative";
  } else if (n === 0) {
    return "zero";
  } else if (n < 10) {
    return "small";
  } else if (n < 100) {
    return "medium";
  } else {
    return "large";
  }
}

function check(): number {
  if (classify(-5) !== "negative") { throw "#1"; }
  if (classify(0) !== "zero") { throw "#2"; }
  if (classify(5) !== "small") { throw "#3"; }
  if (classify(50) !== "medium") { throw "#4"; }
  if (classify(500) !== "large") { throw "#5"; }
  return 0;
}
console.log(check());
