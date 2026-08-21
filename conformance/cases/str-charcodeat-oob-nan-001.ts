// ES §22.1.3.2 String.prototype.charCodeAt step 5: an index outside
// [0, len) answers NaN, not a code unit. The pre-r464 kernel handed
// back 0 there, which is indistinguishable from a real NUL.
const s = "aA0é\u{1F600}z";

for (let i = -2; i <= 8; i = i + 1) {
  console.log(i, s.charCodeAt(i));
}

// NaN is a Number, and it has to behave like one everywhere the
// value can be observed — not just when printed.
console.log(typeof s.charCodeAt(99), Number.isNaN(s.charCodeAt(99)));
console.log(s.charCodeAt(99) + 1, s.charCodeAt(99) > 90, s.charCodeAt(99) === s.charCodeAt(99));
console.log(JSON.stringify(s.charCodeAt(99)), `${s.charCodeAt(99)}`);
console.log(s.charCodeAt(99) | 0, s.charCodeAt(99) >>> 0);

// An annotated `number` slot has to be wide enough to hold it; a
// narrow slot is what used to make this shape fail to build.
let c: number = s.charCodeAt(99);
console.log(c, c + 1);
const arr: number[] = [s.charCodeAt(0), s.charCodeAt(99)];
console.log(arr[0], arr[1]);

// 0-arg form defaults pos to 0 (step 2-3), so the empty string is
// out of range there too.
console.log(s.charCodeAt(), "".charCodeAt(), "".charCodeAt(0));

// The Substr (view) receiver shares the answer.
const sub = s.substring(2, 4);
console.log(sub.charCodeAt(-1), sub.charCodeAt(0), sub.charCodeAt(2), sub.charCodeAt());

// The Any tier already answered NaN before r464; it still does.
const a: any = s;
console.log(a.charCodeAt(99), a.charCodeAt(0));

// ToIntegerOrInfinity on the index runs before the range test.
console.log(s.charCodeAt(1.9), s.charCodeAt(-0.5), s.charCodeAt(NaN), s.charCodeAt(Infinity));
