// Adapted from test262: language/statements/for/* — nested loops, sum of
// products, verifies inner loop's induction variable is scoped properly.
function check(): number {
  let total: number = 0;
  for (let i: number = 1; i <= 5; i = i + 1) {
    for (let j: number = 1; j <= 5; j = j + 1) {
      total = total + i * j;
    }
  }
  // sum_{i=1..5} sum_{j=1..5} i*j = (sum i) * (sum j) = 15 * 15 = 225
  if (total !== 225) { throw "#1"; }
  return 0;
}
console.log(check());
