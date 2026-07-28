// BigInt through the any lane: a BigInt pair rides the bigint
// kernels, a mixed BigInt/Number pair throws (ES ToNumeric
// dispatch), and comparison mixes legally by mathematical value
// (pre-fix every shape here was silent no-output or SIGSEGV: the
// BigInt cell fell into the object ToPrimitive machinery)
const p: any = 3;
try { console.log(2n - p) } catch (e) { console.log("sub caught") }
try { console.log(2n * p) } catch (e) { console.log("mul caught") }
try { console.log(2n + p) } catch (e) { console.log("add caught") }
try { console.log(2n & p) } catch (e) { console.log("and caught") }
console.log(2n < p);
console.log(2n > p);

const a: any = 6n;
const b: any = 4n;
console.log(a + b);
console.log(a - b);
console.log(a * b);
console.log(a / b);
console.log(a % b);
console.log(a ** b);
console.log(a & b);
console.log(a | b);
console.log(a ^ b);
console.log(a << 1n);
console.log(a >> 1n);
console.log(~a);
console.log(a < b);
console.log(a >= b);

// BigInt beside a string stays legal concat / template
console.log("x" + a);
console.log(`${b}`);

// `**` — the rotation-240 checker guard is retired: a BigInt pair
// rides the pow kernel, a mixed pair throws catchably
try { console.log(2n ** p) } catch (e) { console.log("pow caught") }
const e2: any = 3n;
console.log(2n ** e2);
