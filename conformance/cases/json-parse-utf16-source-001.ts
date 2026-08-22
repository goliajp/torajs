// `JSON.parse` reads its source by code unit. A Str payload is
// Latin-1 or UTF-16 LE post-P11.1-S2, with `length` counting units
// either way, so reading `length` bytes handed a UTF-16 source half
// its own payload — every round trip through a string holding a
// character above U+00FF threw SyntaxError on the opening bracket.
// A `\uXXXX` escape was also truncated to its low eight bits.
const wide = "\u4e2d";
const acute = "\u00e9";

console.log(JSON.parse(JSON.stringify([wide]))[0]);
console.log(JSON.parse(JSON.stringify([acute, wide])).join("|"));
console.log(JSON.parse('["\\u4e2d"]')[0]);
console.log(JSON.parse('["\\ud83d\\ude00"]')[0]);
console.log(JSON.parse('["\\u00e9"]')[0]);

const round: any = JSON.parse(
  JSON.stringify({ k: wide, "\u00e9": 1, n: 2.5, b: true, z: null, a: [1, acute] }),
);
console.log(JSON.stringify(round));
console.log(round.k, round.k.length, round.k.charCodeAt(0));

// Numbers, keywords and whitespace still parse out of a UTF-16
// source — every one of those tokens is ASCII sitting in two-byte
// units there.
const nums: any = JSON.parse(JSON.stringify([wide, 12345, -3.5e2, true, false, null]));
console.log(nums[1], nums[2], nums[3], nums[4], nums[5]);
console.log(JSON.parse('  [ 1 , 2 ]  ')[1]);

// Nested, and a non-ASCII key.
console.log(JSON.parse(JSON.stringify({ "\u4e2d\u6587": [acute] }))["\u4e2d\u6587"][0]);

// Trailing garbage after a UTF-16 source is still a SyntaxError.
try {
  JSON.parse(JSON.stringify([wide]) + "x");
} catch (e: any) {
  console.log("threw", e instanceof SyntaxError);
}

// The typed lane drives the same token helpers.
const arr: string[] = JSON.parse(JSON.stringify([wide, acute]));
console.log(arr[0], arr[1], arr.length);
