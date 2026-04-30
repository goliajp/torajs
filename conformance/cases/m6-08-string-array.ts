let xs: string[] = [];
xs.push("a"); xs.push("b"); xs.push("c");
let ys: string[] = xs.map((s: string): string => s + "!");
console.log(ys[0]);
console.log(ys[1]);
console.log(ys[2]);
console.log(ys.join(","));
