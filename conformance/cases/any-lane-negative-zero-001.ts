// §6.1.6.1.1 unary minus flips the SIGN. The any lane spelled it
// `0 - x`, an identity that is right at every input but one: IEEE
// gives (+0) - (+0) = +0. So every zero that reached the any lane
// came back +0, while the typed `-0` had been right all along. The
// multiply path lost it the other way: a -0 product round-tripped
// through the integer immediate, where i64 has no sign for it and
// `0 as f64 == -0.0` hides the difference from the round-trip test.
//
// `1 / x` is the detector here rather than `Object.is`, which has its
// own mixed-lane answer (registered separately).
const z: any = 0;
const zs: any = "0";
const zd: any = new Date(0);
const mz: any = -0;
const one: any = -1;

console.log(1 / -z, 1 / -zs, 1 / -zd);
console.log(1 / -mz, 1 / -(-0));
console.log(1 / (one * 0), 1 / (z * -1), 1 / (one * z));

// the typed spellings, unchanged
let t = 0;
console.log(1 / -0, 1 / -t, 1 / -(0), 1 / (0 * -1));
console.log(Object.is(-0, -0), Object.is(-t, -0));

// a signed zero is still equal to zero, and non-zero negation is
// untouched by the integer-path guard
console.log(one * 0 === 0, -z === 0, -z == 0, (one * 0) < 1);
console.log(-(5 as any), -(2.5 as any), -(-5 as any), -(1e21 as any), -(0.5 as any));
console.log(typeof -z, typeof (one * 0), String(-z), `${one * 0}`);
console.log((one * 0).toFixed(1), (-z).toString());
