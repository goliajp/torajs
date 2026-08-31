// Rotation 543 — §22.1.3.{23,24} step 4 and §B.2.2.1 step 5 all read
// an `undefined` second slot as "to the end of the string", not as
// ToIntegerOrInfinity's 0. The STATIC spelling already answered that
// (the lane loads the receiver's length for a literal `undefined`); a
// value that only turns out to be undefined at RUN time fell through
// to the plain coercion and answered "".
//
// Found downstream of this rotation's mixed-element-type fix, which
// made `[...numbers, undefined]` actually hold `undefined` instead of
// `0`. test262's substr coverage builds its argument matrix out of
// exactly that array, and had been passing only because its own
// reference implementation — running in tr too — read the same wrong
// `0`. Both sides wrong, agreeing.
//
// NaN must not take this path (`slice(0, NaN)` is ""), so the test is
// on the any TAG rather than on the coerced number. `null` likewise
// stays ToIntegerOrInfinity's 0.
const u: any = undefined;
console.log(JSON.stringify("abcdef".substr(2, u)));
console.log(JSON.stringify("abcdef".slice(1, u)), JSON.stringify("abcdef".substring(1, u)));
console.log(JSON.stringify("abcdef".substr(-2, u)));

const n: any = 3;
console.log("abcdef".slice(1, n), "abcdef".substring(1, n), "abcdef".substr(1, n));

const s: any = "3";
console.log("abcdef".slice(1, s), "abcdef".substr(1, s));

const q: any = NaN;
console.log(JSON.stringify("abcdef".slice(1, q)), JSON.stringify("abcdef".substr(1, q)));

const z: any = null;
console.log(JSON.stringify("abcdef".slice(1, z)), JSON.stringify("abcdef".substr(1, z)));

const m: any = -2;
console.log("abcdef".slice(1, m), "abcdef".substring(1, m), JSON.stringify("abcdef".substr(1, m)));

const o: any = {
  valueOf(): number {
    return 3;
  },
};
console.log("abcdef".slice(1, o), "abcdef".substr(1, o));

console.log("abcdef".slice(1, 4), "abcdef".substring(1, 4), "abcdef".substr(1, 4));
console.log("abcdef".slice(1), "abcdef".substring(1), "abcdef".substr(1));
console.log("abcdef".slice(1, undefined), "abcdef".substring(1, undefined));

const ns = [1, 2];
const L = [...ns, undefined];
console.log("ab".substr(0, L[2]));
