// The last two refusals in a 41-lane sweep of the ToNumber family.
// §21.3.2.18 step 2 coerces every hypot element; §21.1.3.6 step 2
// coerces toString's radix. Both admitted Number and Undefined by
// name and refused the rest.

const sixteen: any = 16;
const sixteenStr: any = "16";
const undef: any = undefined;
const nan: any = NaN;
const obj: any = {};

console.log(Math.hypot(sixteenStr, "4"));
console.log(Math.hypot("3", "4"), Math.hypot(null, 4), Math.hypot(true, 0));
console.log(Math.hypot(obj, 1));
console.log(Math.hypot(undef, 1));

// The variadic arities and the statically-undefined fold are unchanged.
console.log(Math.hypot(), Math.hypot(3), Math.hypot(3, 4), Math.hypot(3, 4, 12));
console.log(Math.hypot(undefined, 1));

// The radix is where `undefined` and NaN part ways: undefined means
// radix 10, NaN is a RangeError. So the any box is asked for its tag,
// not for its number.
console.log((255).toString(sixteen));
console.log((255).toString(sixteenStr));
console.log((255).toString(undef));
try {
  console.log((255).toString(nan));
} catch (e) {
  console.log("throw", e instanceof RangeError);
}
console.log((255).toString(sixteen, 0));

// Typed spellings and the RangeError band are unchanged.
console.log((255).toString(16), (255).toString(), (255).toString(undefined));
console.log((255).toString(16.9), (255).toString(2), (255).toString(36));
try {
  console.log((255).toString(1));
} catch (e) {
  console.log("throw", e instanceof RangeError);
}
console.log((255.5).toString(16));
