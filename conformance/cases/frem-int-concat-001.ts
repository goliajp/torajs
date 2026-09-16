// Remainder of an integer counter fed straight into a string concat,
// on both sides of the non-negative range the integer lane needs.
let out: string[] = [];
for (let i = 0; i < 10; i = i + 1) {
  out.push("k" + (i % 4));
}
console.log(out.join(" "));
out = [];
for (let i = -6; i < 3; i = i + 1) {
  out.push("n" + (i % 3));
}
console.log(out.join(" "));
let big = 9007199254740990;
for (let j = 0; j < 2; j = j + 1) {
  console.log("b" + (big % 1000), "c" + big);
  big = big + 1;
}
console.log(String(-0), "" + (-0), `${-0}`, [-0].join());
console.log(4095, -4095, 2 ** 53 - 1, -(2 ** 53) + 1, 2 ** 53, 1e20, 1e21, 123456789012);
console.log("x" + 1e20, "y" + (2 ** 60), "z" + (-123456789));
