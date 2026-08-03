// §22.2.6.8/.11/.12/.13/.14 — the RegExp @@match / @@matchAll /
// @@replace / @@search / @@split protocol methods reify off a RegExp
// receiver and index-call with the receiver in place
// (`re[Symbol.split](s)` ≡ `s.split(re)` with the operand order
// flipped).
const s = "a,b,c";
console.log(JSON.stringify(/,/[Symbol.split](s)));
console.log(JSON.stringify(/,/[Symbol.split](s, 2)));
console.log(JSON.stringify(/b/[Symbol.match](s)));
console.log(/b/[Symbol.search](s));
console.log(/b/[Symbol.replace](s, "X"));
const it = /[abc]/g[Symbol.matchAll](s);
console.log(JSON.stringify([...it].map((m) => m[0])));

// The reified cell reads as a function, and a plain-string separator
// path through .split stays on the fast lane.
console.log(typeof /x/[Symbol.match]);
console.log(JSON.stringify("a,b".split(/,/)));

// A global-flag @@match collects all matches like s.match(re).
console.log(JSON.stringify(/[abc]/g[Symbol.match](s)));
