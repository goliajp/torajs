let s: number = 0;
for (let i: number = 0; i < 100; i = i + 1) {
  if (i % 2 === 0) {
    continue;
  }
  if (i > 15) {
    break;
  }
  s = s + i;
}
console.log(s);
