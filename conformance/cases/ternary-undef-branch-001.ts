// S2.27 (RFC 20260727-dstr-assignment 刀 5) — a ternary with an
// Undefined branch against any other type widens to Any instead of
// rejecting ("branches differ"). This is the destructuring default's
// guard ternary over a MONOMORPHIC Array<Undefined> source, in both
// declaration and assignment forms.

// declaration form over [undefined]
let [du = 13] = [undefined];
console.log(du); // 13

// assignment form over [undefined]
let u;
[u = 13] = [undefined];
console.log(u); // 13

// explicit null keeps null (the Null arm predates this blade)
let n;
[n = 5] = [null];
console.log(n); // null

// direct ternaries with an undefined branch, both polarities
let t = [1];
let a = t.length > 1 ? undefined : 42;
console.log(a); // 42
let b = t.length > 0 ? undefined : 42;
console.log(b); // undefined
