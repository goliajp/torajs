// Math.min/max dynamic spread (chunk 686)
const arr: number[] = [3, 41.5, 42, 7];
console.log(Math.max(...arr));
console.log(Math.min(...arr));
// prefix + spread
console.log(Math.max(100, ...arr));
console.log(Math.min(-1, ...arr));
// empty array -> spec identity
const empty: number[] = [];
console.log(Math.max(...empty));
console.log(Math.min(...empty));
// single element
const one: number[] = [5];
console.log(Math.max(...one));
// static regression (no spread)
console.log(Math.max(1, 2, 3));
console.log(Math.min(4, 2));
