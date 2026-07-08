// per-iteration capture: each iteration's closure sees that iteration's i
let m = (x: number) => x;
for (let i = 0; i < 1; i++) {
  m = (x: number) => x + i;
}
console.log(m(5));
// classic: closures from 3 iterations hold 0,1,2
let a = (x: number) => x;
let b = (x: number) => x;
let c = (x: number) => x;
for (let i = 0; i < 3; i++) {
  if (i === 0) {
    a = (x: number) => x + i;
  }
  if (i === 1) {
    b = (x: number) => x + i;
  }
  if (i === 2) {
    c = (x: number) => x + i;
  }
}
console.log(a(10), b(10), c(10));
// body write propagates to the next iteration via the copy-back
let seen = "";
let g = () => 0;
for (let i = 0; i < 6; i += 1) {
  seen = seen + i;
  i += 1;
  g = () => i;
}
console.log(seen, g());
// break: the abandoned iteration's box releases; captured value holds
let h = () => 0;
for (let i = 0; i < 100; i++) {
  h = () => i;
  if (i === 2) {
    break;
  }
}
console.log(h());
// no-capture loop unchanged
let sum = 0;
for (let i = 0; i < 5; i++) {
  sum += i;
}
console.log(sum);
