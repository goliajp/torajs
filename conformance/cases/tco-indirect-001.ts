// Tail calls whose callee is read at run time: out of a closure cell
// (a function that names itself from inside a closure that also
// captures a variable), out of an object member, or out of a tagged
// template's tag (ES2015 proper tail calls,
// sec-static-semantics-hascallintailposition). The pending-throw check
// after such a call only hands a throw on, so it does not stop the
// frame from being reused; 100k frames overflow the stack otherwise.
"use strict";
const DEPTH = 100000;

// the callee is the closure's own cell
const cellSelf = (function () {
  let rounds = 0;
  function f(n: any): any {
    if (n === 0) return rounds;
    rounds += 1;
    return f(n - 1);
  }
  return f;
})();
console.log(cellSelf(DEPTH));

// the callee is an object member, reassigned after the object is made
type Ops = { step: (n: any) => any };
const ops: Ops = { step: (n: any): any => n };
ops.step = (n: any): any => (n === 0 ? "member" : ops.step(n - 1));
console.log(ops.step(DEPTH));

// a tagged template whose tag is the function itself
const tagged = (function () {
  let seen = 0;
  function f(_: any, n: any): any {
    if (n === 0) return seen;
    seen += 1;
    return f`${n - 1}`;
  }
  return f;
})();
console.log(tagged(null, DEPTH));

// a throw at the bottom passes up through every reused frame
const deepThrow = (function () {
  let rounds = 0;
  function f(n: any): any {
    if (n === 0) throw new Error("bottom after " + rounds);
    rounds += 1;
    return f(n - 1);
  }
  return f;
})();
try {
  deepThrow(DEPTH);
} catch (e: any) {
  console.log("caught", e.message);
}

// shapes that keep their frame (the call is not the last thing that
// runs); the depth stays small and the results must still be right

// the call sits in a try whose catch handles what the callee throws
const inTry = (function () {
  let rounds = 0;
  function f(n: any): any {
    if (n === 0) throw new Error("inner " + rounds);
    rounds += 1;
    try {
      return f(n - 1);
    } catch (e: any) {
      return n + ":" + e.message;
    }
  }
  return f;
})();
console.log(inTry(1000));

// a finally runs after the call returns
const inFinally = (function () {
  let ran = 0;
  function f(n: any): any {
    if (n === 0) return ran;
    try {
      return f(n - 1);
    } finally {
      ran += 1;
    }
  }
  return { f, ran: () => ran };
})();
console.log(inFinally.f(1000), inFinally.ran());

// a finally that returns discards the throw the callee left pending
const finallyReturns = (function () {
  let rounds = 0;
  function f(n: any): any {
    if (n === 0) throw new Error("dropped");
    rounds += 1;
    try {
      return f(n - 1);
    } finally {
      if (n === 1) return "kept " + rounds;
    }
  }
  return f;
})();
console.log(finallyReturns(1000));

// fresh strings and objects as arguments, released after the call
const freshStr = (function () {
  let rounds = 0;
  function f(n: any, s: any): any {
    if (n === 0) return s.length + rounds;
    rounds += 1;
    return f(n - 1, s + (n % 10));
  }
  return f;
})();
console.log(freshStr(1000, ""));

const freshObj = (function () {
  let rounds = 0;
  function f(n: any, o: any): any {
    if (n === 0) return o.total + rounds;
    rounds += 1;
    return f(n - 1, { total: o.total + n });
  }
  return f;
})();
console.log(freshObj(1000, { total: 0 }));

// a closure local to each round, released after the call
const localClosure = (function () {
  let rounds = 0;
  function f(n: any): any {
    if (n === 0) return "local " + rounds;
    rounds += 1;
    const g = () => f;
    return g()(n - 1);
  }
  return f;
})();
console.log(localClosure(1000));
