let xs: number[] = [];
xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);

let evens: number[] = xs.filter((x: number): boolean => x % 2 === 0);
let sum: number = xs.reduce((a: number, x: number): number => a + x, 0);
console.log(evens.length);
console.log(evens[0]);
console.log(evens[1]);
console.log(sum);
