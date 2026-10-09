// float arithmetic on two literals: the folded value must be the one
// the runtime operation produces, edges included
const half = 0.5;
const tenth = 0.1;
const fifth = 0.2;
const negOne = -1.5;
const zero = 0.0;
const inf = 1.0 / zero;

console.log(tenth + fifth);
console.log(tenth + fifth === 0.3);
console.log(1.0 / 3.0);
console.log(half * 6.25 - 0.75);
console.log(-5.5 % 2.0);
console.log(5.5 % -2.0);
console.log(1 / (negOne * zero));
console.log(1 / (-4.0 % 2.0));
console.log(1 / (4.0 % -2.0));
console.log(1.0 / 0.0, -1.0 / 0.0);
console.log(0.0 / 0.0);
console.log(1.5 % 0.0);
console.log(5.25 % inf);
console.log(inf - inf);
console.log(inf * zero);
console.log(1e308 * 10.0);
console.log(5e-324 / 2.0);
console.log(2.5e-324 * 1.0);
console.log(9007199254740992.0 + 1.0);
console.log(0.1 * 3.0);
