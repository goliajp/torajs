// torajs 'number' is i64; bun's number is f64. For values inside the
// f64-safe-int range (≤ 2^53) outputs match. We stay conservative.
let big: number = 1000000;
console.log(big * big);
console.log(big * big * 100);
