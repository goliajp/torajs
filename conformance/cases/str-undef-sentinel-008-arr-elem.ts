// a `string[]` element slot may hold the undefined sentinel (an OOB
// string-index read pushed into the array) — typeof takes the
// two-state runtime branch instead of the static "string" fold, the
// eq fast path declines to the identity-aware compare, and let-init
// aliases inherit the routing (660 residual, is_nullable_str_source
// array-index source).
const s = "abc";
const xs: string[] = ["x"];
xs.push(s[10]);
console.log(typeof xs[0]);
console.log(typeof xs[1]);
console.log(xs[1] === undefined);
console.log(xs[1] === "undefined");
const c = xs[1];
console.log(typeof c);
let cnt = 0;
for (let i = 0; i < 1000; i++) {
  if (typeof xs[1] === "undefined") cnt++;
}
console.log(cnt);
console.log(xs[0].length);
console.log(xs[0] === "x");
