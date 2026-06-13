function f(thunk: () => void): void {
  thunk();
}
f(function() { console.log("anon"); });
f(() => { console.log("arrow"); });
var h = function() { console.log("via-var"); };
f(h);
function g(): void { console.log("named"); }
f(g);
function withRet(cb: () => number): void {
  console.log(cb());
}
withRet(function() { return 42; });
function inner(cb: () => void): void { cb(); }
function outer(t2: () => void): void { inner(t2); }
outer(function() { console.log("transitive"); });
function withArg(op: (x: number) => number): void {
  console.log(op(10));
}
withArg(function(x: number) { return x * 3; });
withArg((x: number) => x + 5);
