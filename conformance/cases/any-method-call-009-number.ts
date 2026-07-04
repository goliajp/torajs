// RFC 20260704 C4-3b — number methods through `any` receivers.
const n: any = 5.12345;
console.log(n.toFixed(2));
console.log(n.toFixed());
console.log(n.toString());
console.log(n.toPrecision(3));
console.log(n.toExponential(1));
console.log(n.toLocaleString());
console.log(n.valueOf());
const i: any = 255;
console.log(i.toString());
console.log(i.toString(16));
console.log(i.toString(2));
console.log(i.toFixed(1));
console.log(i.toExponential());
console.log(i.toPrecision(5));
console.log(i.valueOf());
const big: any = 1234567.891;
console.log(big.toLocaleString());
const neg: any = -0.5;
console.log(neg.toFixed(3));
try {
  i.toFixed(101);
} catch (err) {
  console.log("range");
}
try {
  i.notANumberMethod();
} catch (err) {
  console.log("threw");
}
