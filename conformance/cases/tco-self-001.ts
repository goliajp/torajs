// RFC 20260810-self-tail-call — named function expression self tail
// calls run in O(1) stack via the egraph rebind loop. 1M depth would
// overflow any frame size without the rewrite (8MB stack / ≥80B frame
// caps out near 100k).
let callCount = 0;
(function f(n: any) {
  if (n === 0) {
    callCount += 1;
    return;
  }
  return f(n - 1);
})(1000000);
console.log(callCount);

// parallel-move shape: both args swap through temps on rebind
let final_b = -1;
(function g(a: any, b: any) {
  if (a === 0) {
    final_b = b;
    return;
  }
  return g(b - 1 < a - 1 ? b - 1 : a - 1, a - 1);
})(200000, 200000);
console.log(final_b);

// missing-arg rebind: the recursive call drops an argument; the argc
// normalization re-runs on loop entry and binds it undefined
(function h(n: any, acc: any) {
  if (acc === undefined) {
    return h(n);
  }
  if (n === 0) {
    console.log("h", acc);
    return;
  }
  return h(n - 1, acc);
})(300000, 7);

// heap-arg passthrough: string arg alternates between rebuilt and
// passed-through — exercises the retain/release handoff on rebind
let last = "";
(function s(n: any, msg: any) {
  if (n === 0) {
    last = msg;
    return;
  }
  if (n % 4 === 0) {
    return s(n - 1, "long-heap-string-payload-" + n);
  }
  return s(n - 1, msg);
})(100000, "seed-long-heap-string-payload");
console.log(last.length, last.slice(0, 4));

// mutual recursion keeps the original call path (guard false at
// runtime — the loaded cell is the sibling, not self)
function isEven(n: any): any {
  return n === 0 ? true : isOdd(n - 1);
}
function isOdd(n: any): any {
  return n === 0 ? false : isEven(n - 1);
}
console.log(isEven(100), isOdd(101));

// return-value tail call: result of the self call is the return value
const sum = (function add(n: any, acc: any): any {
  if (n === 0) {
    return acc;
  }
  return add(n - 1, acc + n);
})(500000, 0);
console.log(sum);
