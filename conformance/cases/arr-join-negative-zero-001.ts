// `Array.prototype.join` reads every element through ToString, and
// ES §6.1.6.1.20 says the string for -0 is "0" — the sign belongs to
// the value, not to its text. The console inspector is the path that
// keeps it, which is why the two format through different helpers.
const z = [-0];
console.log(z.join(","));
console.log(z.toString());
console.log(String(z));
console.log(`${z}`);
console.log([-0].join(""));
console.log([1, -0, 2.5].join(","));
console.log([NaN, Infinity, -Infinity, -0].join(","));
console.log([0.0].join(","), [-0.0, 1].join(","));
console.log(JSON.stringify(z));

// The any lane already agreed; it must keep agreeing.
const anyz: any[] = [-0];
console.log(anyz.join(","));

// Inspection keeps the sign, and the value itself is untouched.
console.log(z[0], z[0] === 0, Object.is(z[0], -0));

// Ordinary negatives are not zeros and keep their sign.
console.log([-1, -0.5].join(","));
