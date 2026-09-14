// §27.2.1.3.2 step 7 — a resolving function called with its own
// promise rejects that promise with a TypeError. tr adopted the
// promise into itself instead (an attach to its own settle) and it
// stayed pending forever, so neither handler ran (rotation 650; the
// handler-return route in torajs-promise already had the check).
var r: any, j: any;
var p: any = new Promise(function (res: any, rej: any) { r = res; j = rej; });
r(p);
p.then(function () { console.log("fulfilled?"); }, function (e: any) { console.log("rejected", e instanceof TypeError); });
var P: any = function (executor: any) { executor(r, j); return p; };
var q: any = (Promise.resolve as any).call(P, p);
console.log(q === p);
q.then(function () { console.log("q fulfilled?"); }, function (e: any) { console.log("q rejected", e instanceof TypeError); });
var r2: any;
var p2: any = new Promise(function (res: any) { r2 = res; });
r2(Promise.resolve(3));
p2.then(function (v: any) { console.log("p2", v); });
new Promise(function (res: any) { res(1); res(2); }).then(function (v: any) { console.log("first wins", v); });
