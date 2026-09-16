// §13.15.5.5 is recursive. An outer pattern with no reference of its
// own still owes the interleaving when an INNER pattern has one: the
// outer iterator must not be closed before the inner element targets
// have been evaluated.
const log: string[] = [];
function f(): string {
  log.push("f");
  return "p";
}
function g(): string {
  log.push("g");
  return "q";
}
function mk(tag: string, values: any[]): any {
  let i = 0;
  const it: any = {
    next: function () {
      log.push(tag + "next");
      if (i >= values.length) {
        return { done: true, value: undefined };
      }
      return { done: false, value: values[i++] };
    },
    return: function () {
      log.push(tag + "ret");
      return {};
    },
  };
  const src: any = {};
  src[Symbol.iterator] = function () {
    return it;
  };
  return src;
}
const a: any = {};
const b: any = {};
[[a[f()], b[g()]]] = mk("o", [mk("i", [1, 2, 3])]);
console.log(log.join(","));
console.log(a.p, b.q);
// an object pattern holds no iterator but can hold the targets whose
// order the enclosing array pattern owes
const c: any = {};
log.length = 0;
[{ k: c[f()] }] = mk("o", [{ k: 42 }]);
console.log(log.join(","), c.p);
