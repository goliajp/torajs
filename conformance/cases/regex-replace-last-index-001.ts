// §22.2.6.11 step 10 — a global replace sets lastIndex to 0 first, and
// its walk ends on a failing exec, so lastIndex reads 0 afterwards, for
// a string replacement, a function replacement and replaceAll.

const g = /b/g;
g.lastIndex = 2;
console.log("abcb".replace(g, "."), g.lastIndex);
g.lastIndex = 3;
console.log("abcb".replace(g, (m: string) => m.toUpperCase()), g.lastIndex);
g.lastIndex = 1;
console.log("abcb".replaceAll(g, "$&$&"), g.lastIndex);
g.lastIndex = 4;
console.log("xyz".replace(g, "-"), g.lastIndex);

// a non-global replace leaves lastIndex where it was
const n = /b/;
n.lastIndex = 3;
console.log("abcb".replace(n, "."), n.lastIndex);

