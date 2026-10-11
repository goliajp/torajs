// defaults run in parameter order (ECMA-262 §10.2.11): once one of them
// runs in the body, the ones after it have to as well
let log: string[] = [];
function t(s: string): number {
  log.push(s);
  return 1;
}
function f(a: any = t("a"), b: number = t("b")) {
  return log.join(",");
}
console.log("top level:", f());
log = [];
console.log("explicit undefined:", f(undefined, undefined));
log = [];
console.log("second passed:", f(undefined, 2));

function g(a: any = t("a"), b: string = "s" + t("b"), c: number[] = [t("c")]) {
  return log.join(",") + "|" + b + c.length;
}
log = [];
console.log("three lanes:", g());

function nested() {
  function h(a: any = t("a"), b: number = t("b")) {
    return log.join(",");
  }
  log = [];
  return h();
}
console.log("nested:", nested());

class C {
  v: string;
  constructor(a: any = t("a"), b: number = t("b")) {
    this.v = log.join(",");
  }
}
log = [];
console.log("constructor:", new C().v);
