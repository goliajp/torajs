// A builtin method value bound BEFORE an own `then` was installed
// must keep calling the builtin: `bt` is Promise.prototype.then with
// `q` as its receiver, not "whatever `q.then` names when called".
// The any-lane promise arm consulted the own bag a second time,
// ignoring the re-dispatch flag the bound cell sets, so `bt(...)`
// re-entered the override — test262's `Promise/any/invoke-then`
// forwards through exactly such a bound `then` and recursed until
// the stack ran out (rotation 651).
let q: any = Promise.resolve(7);
let bt = q.then.bind(q);
q.then = function (r: any, j: any) { return "own"; };
console.log(typeof bt((v: any) => v), q.then(1, 2));

// The override forwarding to the bound builtin: the combinators'
// per-element Invoke reaches the override once, which settles the
// outer through the real `then`.
let p: any = Promise.resolve(3);
let boundThen = p.then.bind(p);
let cnt = 0;
p.then = function (r: any, j: any) { cnt++; return boundThen(r, j); };
Promise.any([p]).then((v: any) => console.log("any", v, cnt));
Promise.all([p]).then((v: any) => console.log("all", JSON.stringify(v), cnt));
Promise.race([p]).then((v: any) => console.log("race", v, cnt));
Promise.allSettled([p]).then((v: any) => console.log("allSettled", JSON.stringify(v), cnt));

// A reified builtin stored back as the own `then` is the builtin.
let s: any = Promise.resolve("s");
s.then = Promise.prototype.then;
s.then((v: any) => console.log("reified", v));

// An accessor `then` runs its getter, and its answer is called with
// the promise as `this`.
let g: any = Promise.resolve(9);
let gets = 0;
Object.defineProperty(g, "then", { get: function () { gets++; return function (this: any, r: any) { console.log("getter then", this === g); r(0); }; } });
Promise.all([g]).then((_: any) => console.log("gets", gets));
