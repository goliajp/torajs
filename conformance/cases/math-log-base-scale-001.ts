// log2 / log10 written as `log(x) / ln(base)` make the scale change
// fight the decomposition: `log` has already folded `k · ln2` into
// its answer, so dividing puts a value as large as 744 through the
// rounding. Keeping `k` outside sends only the mantissa through the
// transcendental step, whose own log is at most 0.5 in magnitude.
console.log(Math.log10(0.0001), Math.log10(1e-300), Math.log10(Number.MIN_VALUE));
console.log(Math.log10(2), Math.log10(1000), Math.log10(1e22));
console.log(Math.log2(5), Math.log2(7), Math.log2(Math.E) === Math.LOG2E);

// Whole exponent ranges, so a fix at one point that breaks a stretch
// elsewhere cannot hide.
let bad2 = 0;
for (let i = -1074; i < 1024; i++) {
  if (Math.log2(Math.pow(2, i)) !== i) { bad2 = bad2 + 1 }
}
let bad10 = 0;
for (let i = -300; i < 301; i++) {
  if (Math.log10(Math.pow(10, i)) !== i) { bad10 = bad10 + 1 }
}
console.log("log2 exact over full range:", bad2 === 0, "log10 exact over 10^i:", bad10 === 0);
