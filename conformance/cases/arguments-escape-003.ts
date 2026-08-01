// RFC 20260801-arguments-escape-face knife 3a — top-level named fn:
// bare `arguments` escape with a uniform static call-site argc.

// 1. return escape, single call site, over-arity
function getArgs() { return arguments; }
const a = getArgs([1], [2]);
console.log(a.length, a[0][0], a[1][0]);

// 2. declared params + extras
function withDeclared(x: any) { return arguments; }
const b = withDeclared("x", "y");
console.log(b.length, b[0], b[1]);

// 3. assign escape
let cap: any = null;
function capIt() { cap = arguments; }
capIt(5, 6, 7);
console.log(cap.length, cap[2]);

// 4. pass-to-call escape
function len2(o: any) { return o.length; }
function passer() { return len2(arguments); }
console.log(passer(1, 2, 3, 4));

// 5. length + escape in the same body
function mix() { const n = arguments.length; cap = arguments; return n; }
console.log(mix(9), cap.length, cap[0]);

// 6. under-arity: declared 3, passed 1
function under(p: any, q: any, r: any) { return arguments; }
const u = under("only");
console.log(u.length, u[0]);

// 7. two call sites with the same argc
function multi() { return arguments; }
const m1 = multi(1, 2);
const m2 = multi(3, 4);
console.log(m1[0], m1[1], m2[0], m2[1]);

// 8. for-of over arguments in a named fn
function iter() { let s = 0; for (const v of arguments) s = s + v; return s; }
console.log(iter(10, 20, 30));
