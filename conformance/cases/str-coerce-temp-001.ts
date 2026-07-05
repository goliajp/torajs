// RFC 20260705 chunk 546 — coercion temps minted by the `+` concat
// family and the s.concat left-fold are dropped post-concat.
let n = 0;
let s = "v" + 42;
console.log(s);
let f = "f" + 1.5;
console.log(f);
let b = "b" + true;
console.log(b);
let nl = "n" + null;
console.log(nl);
let ud = "u" + undefined;
console.log(ud);
let arr = [1, 2];
let aj = "a" + arr;
console.log(aj);
let big = "g" + 10n;
console.log(big);
let multi = "x".concat("y", "z", "w");
console.log(multi);
"discard" + 7;
let sub = "hello world".slice(0, 5);
let sc = sub + 99;
console.log(sc);
console.log(n);
