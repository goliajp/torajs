// §22.2.6.11 step 14.e passes `undefined` for a capture group that did
// not participate. A replacer called through the boxed entry (one that
// returns nothing, or declares an `any` return) received the text
// "undefined" in its string-typed capture parameter.
let actual: any = 1;
"x".replace(/x(.)?/g, function (m, group) {
  actual = group;
});
console.log(typeof actual, actual === undefined);
let a2: any = 1;
console.log("x".replace(/x(.)?/, (m: string, g: string): any => { a2 = g; return 5; }), typeof a2);
let a3: any = 1;
console.log("x".replace(/x(.)?/, function (m, g) { a3 = g; return "R"; }), typeof a3);
