// §27.2.4 static-slot patch consult (rotation 448) — a patch stored
// through the any lane on the Promise constructor cell must be
// honored by the STATIC `Promise.resolve` / `Promise.reject` call
// sites (the pre-fix lanes silently kept the builtin), and an
// unpatched program's sites stay on the typed fast path.
var pre = Promise.resolve(1);
pre.then(function (x: any) { console.log("pre", x); });

var obj: any = Promise;
obj.resolve = function (v: any) {
  console.log("patched", v);
  return new Promise(function (res: any, rej: any) { rej("from-patch:" + v); });
};

var p = Promise.resolve(8);
p.catch(function (e: any) { console.log("caught", e); });

var z = Promise.resolve();
z.catch(function (e: any) { console.log("z-caught", e); });

obj.reject = function (v: any) {
  console.log("patched-reject", v);
  return new Promise(function (res: any, rej: any) { res("swapped:" + v); });
};
var r = Promise.reject(3);
r.then(function (x: any) { console.log("r-then", x); });

console.log("sync-after");
