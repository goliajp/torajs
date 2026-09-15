// §27.2.5.1 — `catch` is Invoke(this, "then", «undefined, onRejected»):
// an own `then` on the cell runs instead of the builtin, on both lanes.
var p: any = Promise.resolve(1);
p.then = function(a: any, b: any) { console.log("own then", arguments.length, typeof a, typeof b, this === p); return 42; };
console.log("any catch ->", p.catch(function(){}));
console.log("any catch() ->", p.catch());

const q = Promise.resolve(3);
(q as any).then = function(a: any, b: any) { console.log("typed own", arguments.length, typeof a, typeof b); return Promise.resolve(7); };
q.catch((e) => 0).then((v) => console.log("typed catch ->", v));

var u: any = Promise.resolve(1);
u.then = undefined;
try { u.catch(function(){}); console.log("no throw"); } catch (e) { console.log("then undefined:", e instanceof TypeError); }

var n: any = Promise.resolve(1);
n.other = 1;
n.catch(function(){}).then(function(v: any) { console.log("bag without then keeps builtin", v); });

var seen: string[] = [];
var orig: any = Promise.prototype.then;
(Promise.prototype as any).then = function(a: any, b: any) { seen.push("patched"); return orig.call(this, a, b); };
var r: any = Promise.reject(new Error("x"));
r.catch(function(e: any) { seen.push("handled " + e.message); }).then(function() { console.log(JSON.stringify(seen)); });
