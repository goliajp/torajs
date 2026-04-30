let xs: number[] = [];
xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);

let doubled: number[] = xs.map((x: number): number => x * 2);
console.log(doubled[0]);
console.log(doubled[2]);
console.log(doubled[4]);
