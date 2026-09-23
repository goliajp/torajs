// §21.4.3.4 step 1 is ToNumber(year); §18 hands the missing argument
// undefined, so the result is NaN. The trailing slots keep the
// clause's own "else let x be" values.
console.log("0", Date.UTC());
console.log("1", Date.UTC(2020));
console.log("2", Date.UTC(2020, 0));
console.log("3", Date.UTC(2020, 0, 2));
console.log("7", Date.UTC(2020, 0, 2, 3, 4, 5, 6));
