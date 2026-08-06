// §13.15.5.4 AssignmentRestProperty — object rest in a destructuring
// ASSIGNMENT (not a declaration). It takes every own enumerable key the
// earlier fields did not name, and unlike the declaration form its
// target can be any assignment target, not just a fresh binding.

const vals = { foo: 1, bar: 2, baz: 3 };

let b: any, rest: any;
({ foo: b, ...rest } = vals);
console.log(b, JSON.stringify(rest));

// the rest target may be a member expression
const holder: any = {};
({ foo: holder.f, ...holder.r } = vals);
console.log(JSON.stringify(holder));

// rest alone copies every own enumerable key
let only: any;
({ ...only } = vals);
console.log(JSON.stringify(only));

// several named fields, then the remainder
let m: any, n: any, tail: any;
({ foo: m, bar: n, ...tail } = vals);
console.log(m, n, JSON.stringify(tail));
