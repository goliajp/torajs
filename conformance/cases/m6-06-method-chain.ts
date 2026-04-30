let xs: number[] = [];
xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);

// sum of squares of evens: 4 + 16 = 20
let r: number = xs
  .filter((x: number): boolean => x % 2 === 0)
  .map((x: number): number => x * x)
  .reduce((a: number, x: number): number => a + x, 0);
console.log(r);
