// A number[] slot holding the undefined sentinel — a hole read into a
// fresh array by spread / Array.from / toReversed / with — prints as
// undefined; a computed NaN still prints NaN.
let a = [1.5, 2.5];
a[4] = 3.5;
console.log([...a]);
console.log(Array.from(a));
console.log(a.toReversed(), a.with(0, 9));
console.log([0 / 0, 1.5], [...a].length);
