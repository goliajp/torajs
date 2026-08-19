// A fn-expr returned from a plain FnDecl / fn-expr body binds `this`
// at ITS call site (§10.2.1.2), not the returning function's — the
// class-member return face's gate, widened to every top-level FnDecl.
function getFn() { return function () { return this; }; }
var f1 = getFn();
console.log(f1() === undefined);

var mk = function () { return function () { return this; }; };
var f2 = mk();
console.log(f2() === undefined);

// the returned fn called as a method — the receiver flows
var host: any = { probe: 7 };
host.m = getFn();
console.log(host.m() === host);
