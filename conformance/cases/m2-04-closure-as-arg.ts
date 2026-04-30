function reduce(xs: number[], f: (n: number) => number): number {
  let s: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    s = s + f(xs[i]);
  }
  return s;
}

function add1(x: number): number { return x + 1; }

let xs: number[] = [];
xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);
console.log(reduce(xs, add1));
