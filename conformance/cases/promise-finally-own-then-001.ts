// §27.2.5.3 — `finally` is Invoke(this, "then", «thenFinally, catchFinally»)
// over two minted closures; an own `then` sees them.
var target: any = new Promise(function(){});
var seen: any = {};
target.then = function(a: any, b: any) { seen.a = a; seen.b = b; seen.n = arguments.length; seen.self = this === target; return 5; };
var result = target.finally(function(){});
console.log(result, seen.n, seen.self, typeof seen.a, typeof seen.b, seen.a.length, JSON.stringify(seen.a.name), seen.b.length);
try { new seen.a(); console.log("ctor ok"); } catch (e) { console.log("thenFinally not a ctor:", e instanceof TypeError); }
var t2: any = new Promise(function(){});
t2.then = function(a: any, b: any) { console.log("non-callable rides both slots:", a, b); return 9; };
console.log(t2.finally(1, 2, 3));

// test262 finally/rejected-observable-then-calls — the sequence
// [1,2,3,4,5] needs catch AND finally to Invoke the own `then`.
var sequence: number[] = [];
var noReason = {};
var no: any = Promise.reject(noReason);
no.then = function() { sequence.push(1); return Promise.prototype.then.apply(this, arguments); };
var yesValue = {};
var yes: any = Promise.resolve(yesValue);
yes.then = function() { sequence.push(4); return Promise.prototype.then.apply(this, arguments); };
no.catch(function(e: any) { sequence.push(2); throw e; })
  .finally(function() { sequence.push(3); return yes; })
  .catch(function(e: any) { sequence.push(5); console.log("reason kept:", e === noReason); })
  .then(function() { console.log(JSON.stringify(sequence)); });

// typed receiver, own `then`
const q = Promise.resolve(3);
(q as any).then = function(a: any, b: any) { console.log("typed own", typeof a, typeof b); return Promise.resolve(8); };
q.finally(() => {}).then((v) => console.log("typed finally ->", v));

// typed receiver, builtin `then` — the kernel keeps its lane
const k = Promise.resolve(11);
k.finally(() => { console.log("fin"); }).then((v) => console.log("typed builtin finally ->", v));
