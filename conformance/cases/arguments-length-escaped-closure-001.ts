// RFC 20260810-indirect-argc-abi L3b ② — a length-only env-first
// body needs no binding-chain admission: every env-first call path
// feeds the S1 hidden argc, so escaped closures (container-stored /
// returned / passed-along — the shapes the value tiers' escape
// analysis used to reject loudly) read the true count too.

// container-stored
const fns = [function () { return arguments.length; }];
console.log(fns[0](1, 2));

// returned through a factory
function mk(): any { return function () { return arguments.length; }; }
const f = mk();
console.log(f(1, 2, 3));

// passed along as a callback
function call(cb: any) { return cb(7, 8); }
console.log(call(function () { return arguments.length; }));

// length-write form rides the synthesized mutable local
const w = [function () { arguments.length = 9; return arguments.length; }];
console.log(w[0](1, 2));
