// globalThis member reads — RFC 20260807-global-object G1: a member
// read of a KNOWN builtin through `globalThis` rewrites to the bare
// name (§9.3; bun answers globalThis.Array === Array true). Mutation
// positions and unknown names stay loud (G2 owns the dynamic surface).
console.log(globalThis.Array === Array);
console.log(globalThis.Math.max(3, 7));
console.log(globalThis.JSON.stringify([1, 2]));
console.log(typeof globalThis.parseInt);
console.log(globalThis.parseInt("42"));
console.log(globalThis.NaN !== globalThis.NaN);
console.log(globalThis.undefined === undefined);
var A = globalThis.Array;
console.log(A.isArray([1]));
console.log("after");
