// §15.1 — a FunctionRestParameter is a BindingRestElement, whose
// right-hand side is a BindingIdentifier OR a BindingPattern. Four
// parameter-list parsers read the rest marker as a reason to stop
// looking for a pattern, so `(...[a, b])` and `(...{length})` were
// parse errors in every position.
function pair(...[a, b]: any[]): number {
  return a + b;
}
console.log(pair(1, 2));

function withPrefix(first: number, ...[b, c]: any[]): number {
  return first + b + c;
}
console.log(withPrefix(1, 2, 3));

function nested(...[a, [b, c]]: any[]): number {
  return a + b + c;
}
console.log(nested(1, [2, 3]));

function counted(...{ length }: any[]): number {
  return length;
}
console.log(counted(1, 2, 3));

function defaulted(...[a = 5]: any[]): number {
  return a;
}
console.log(defaulted(), defaulted(1));

function hole(...[, b]: any[]): number {
  return b;
}
console.log(hole(1, 2));

function tail(...[a, ...r]: any[]): number {
  return a + r.length;
}
console.log(tail(1, 2, 3));

const arrow = (...[a, b]: any[]) => a * b;
console.log(arrow(3, 4));

class K {
  constructor(...[v]: any[]) {
    this.v = v;
  }
  v: any;
  diff(...[a, b]: any[]): number {
    return a - b;
  }
}
console.log(new K(7).v, new K(0).diff(9, 4));
