// String.prototype.search / match over an any-typed pattern that carries
// the builtin RegExp protocol method: the method runs its spec algorithm
// against that receiver (its own `exec`, its `lastIndex`), not the native
// kernel over the pattern's text.
const o: any = {};
o[Symbol.search] = RegExp.prototype[Symbol.search];
o.lastIndex = 0;
o.exec = function (s: string) {
  return { index: s.length * 10 };
};
console.log("abc".search(o));

const m: any = {};
m[Symbol.match] = RegExp.prototype[Symbol.match];
m.flags = "";
m.exec = function (s: string) {
  return ["custom:" + s];
};
console.log(JSON.stringify("xyz".match(m)));

