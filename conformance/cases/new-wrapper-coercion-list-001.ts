// §21.1.1.1 / §22.1.1.1 / §20.3.1.1 — `new Number(x)` / `new String(x)`
// take the same argument domain as the callable forms; with a NewTarget,
// String runs ToString (a Symbol throws). Extra arguments are evaluated
// in order and ignored.
const log: string[] = [];
function t(s: string): number {
  log.push(s);
  return 0;
}
class P {
  valueOf() {
    return 7;
  }
  toString() {
    return "p";
  }
}
const m = new Map([[1, 2]]);
console.log(Number(new Number({ valueOf() { return 2; } })), Number(new Number(new P())));
console.log(Number(new Number(m)), String(new String(m)), String(new String(new P())));
console.log(Number(new Number(10n)), String(new String(/a/g)));
const q: number | undefined = undefined;
console.log(Number(new Number(q)), String(new String(q)));
try {
  new String(Symbol("s"));
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(String(Symbol("d")));
const bw = new Boolean(0, t("c"));
console.log(Number(new Number(1, t("a"))), String(new String("x", t("b"))), bw.valueOf());
console.log(log.join(","));
