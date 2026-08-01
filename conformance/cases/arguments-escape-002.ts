// RFC 20260801-arguments-escape-face knife 2 — closure VALUE form:
// bare `arguments` escaping through the argv face (runtime argc+argv).

// 1. return escape through a binding, varying call arity
const f = function () { return arguments; };
const r1 = f(1, "x", true);
console.log(r1.length, r1[0], r1[1], r1[2]);
const r2 = f();
console.log(r2.length);

// 2. alias binding shares the argv face
const g = f;
const r3 = g("a", 2);
console.log(r3.length, r3[0], r3[1]);

// 3. assign escape from a value closure
let cap: any = null;
const h = function () { cap = arguments; };
h("p", "q", "r");
console.log(cap.length, cap[0], cap[1], cap[2]);

// 4. pass-to-call escape
function len(o: any) { return o.length; }
const k = function () { return len(arguments); };
console.log(k(7, 8));

// 5. typeof arguments is "object"
const t = function () { return typeof arguments; };
console.log(t(1));

// 6. length + escape in the same body rides the argv face
let mlen = 0;
let mcap: any = null;
const m = function () { mlen = arguments.length; mcap = arguments; };
m(10, 20);
console.log(mlen, mcap.length, mcap[1]);

// 7. declared params + extras through the value form
const d = function (a: any) { return arguments; };
const r7 = d("first", "second", "third");
console.log(r7.length, r7[0], r7[2]);
