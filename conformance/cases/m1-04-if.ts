function abs(n: number): number {
  if (n < 0) {
    return 0 - n;
  } else {
    return n;
  }
}
console.log(abs(5));
console.log(abs(-7));
console.log(abs(0));
