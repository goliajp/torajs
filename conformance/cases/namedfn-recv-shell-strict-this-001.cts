// A receiver-first relay shell (`__fwdrecv_<cb>`, minted for a named
// declaration at a thisArg callback position) is transparent: the
// target decides its own `this` binding (a sloppy target coerces to
// the global object, a strict one keeps what the caller passed). The
// fn-expr this-face pass used to take the shell as a face — its
// `__this` user param satisfied the has-this-param probe — and write
// the sloppy prologue into the shell body, which has no directive of
// its own, so in a sloppy file that carries any fn-expr face (the
// property store below is one) a STRICT named callback saw the global
// object where node sees undefined, and a primitive thisArg wrapped
// (rotation 650). bun strips a function-body directive statement in a
// sloppy-goal file, so the oracle here is `.expected` (node-verified).
var T: any = {}; T.k = function (m: string): string { return m; };
var r: any = "unset";
function cb(val, idx, obj) { "use strict"; r = [typeof this, this === undefined]; return true; }
[1].every(cb, undefined); console.log("typed-undef", JSON.stringify(r));
[1].every(cb, 5); console.log("typed-5", JSON.stringify(r));
[1].some(cb); console.log("typed-none", JSON.stringify(r));
var xs: any = [1];
xs.every(cb, undefined); console.log("any-undef", JSON.stringify(r));
xs.every(cb, 5); console.log("any-5", JSON.stringify(r));
xs.map(cb); console.log("any-none", JSON.stringify(r));
function sl(val, idx, obj) { r = [typeof this, this === globalThis]; return true; }
[1].every(sl, undefined); console.log("sloppy-typed-undef", JSON.stringify(r));
[1].every(sl, 5); console.log("sloppy-typed-5", JSON.stringify(r));
xs.every(sl); console.log("sloppy-any-none", JSON.stringify(r));
