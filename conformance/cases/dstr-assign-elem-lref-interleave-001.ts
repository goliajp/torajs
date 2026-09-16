// §13.15.5.5 — lref(0), step(0), put(0), lref(1), step(1), put(1).
// The whole-pattern walk would report next,next,<close>,f,g instead.
const log: string[] = [];
function f(): string {
  log.push("f");
  return "p";
}
function g(): string {
  log.push("g");
  return "q";
}
const iterator: any = {
  next: function () {
    log.push("next");
    return { done: false, value: log.length };
  },
  return: function () {
    log.push("return");
    return {};
  },
};
const src: any = {};
src[Symbol.iterator] = function () {
  return iterator;
};
const a: any = {};
const b: any = {};
[a[f()], b[g()]] = src;
console.log(log.join(","));
console.log(a.p, b.q);
