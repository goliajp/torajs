var prev: any = null;
function tag(t) { prev = t; }
function id(x) { return x; }
tag`a${1}b`;
console.log(prev !== null, prev.length, prev.raw.length);
// extras evaluate for side effects, never bind
var log: string[] = [];
function one(a) { log.push("call:" + a); }
function mk(v: string): string { log.push("arg:" + v); return v; }
one(mk("x"), mk("y"), mk("z"));
console.log(log.join(","));
console.log(id(7, 99));
