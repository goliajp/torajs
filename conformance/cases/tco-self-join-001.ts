// Self tail calls whose result reaches the return through a join: the
// call is the taken operand of `?:`, `&&`, `||` or `??` (ES2015
// proper tail calls, sec-static-semantics-hascallintailposition), or
// sits in a catch block whose binding is released on the way out.
// 100k frames overflow the stack unless each round reuses one.
const DEPTH = 100000;

// number results
const condNum = function f(n: any, acc: any): any {
  return n === 0 ? acc : f(n - 1, acc + (n % 3));
};
console.log(condNum(DEPTH, 0));

const condElse = function f(n: any): any {
  return n > 0 ? f(n - 1) : n;
};
console.log(condElse(DEPTH));

const andNum = function f(n: any, acc: any): any {
  if (n === 0) return acc;
  return n > 0 && f(n - 1, acc + 2);
};
console.log(andNum(DEPTH, 1));

const orNum = function f(n: any, acc: any): any {
  if (n === 0) return acc;
  return n < 0 || f(n - 1, acc - 1);
};
console.log(orNum(DEPTH, 0));

const nullishNum = function f(n: any, acc: any): any {
  if (n === 0) return acc;
  return (n === -1 ? 7 : null) ?? f(n - 1, acc + 1);
};
console.log(nullishNum(DEPTH, 0));

// heap results: the returned value is an object or a string built at
// the bottom and handed back through every join
const condObj = function f(n: any, tag: any): any {
  return n === 0 ? { tag, depth: DEPTH } : f(n - 1, tag);
};
const o = condObj(DEPTH, "cond");
console.log(o.tag, o.depth);

const andStr = function f(n: any, s: any): any {
  if (n === 0) return s + "!";
  return s.length > 0 && f(n - 1, s);
};
console.log(andStr(DEPTH, "and"));

const orArr = function f(n: any, xs: any): any {
  if (n === 0) return xs.concat([n]);
  return xs.length === 0 || f(n - 1, xs);
};
console.log(orArr(DEPTH, [1, 2]).join(","));

const nullishObj = function f(n: any, box: any): any {
  if (n === 0) return box;
  return undefined ?? f(n - 1, box);
};
console.log(nullishObj(DEPTH, { v: 42 }).v);

// an argument that is a fresh heap value every round
const condFresh = function f(n: any, acc: any): any {
  return n === 0 ? acc.join("") : f(n - 1, n % 25000 === 0 ? acc.concat(["x"]) : acc);
};
console.log(condFresh(DEPTH, []));

// the catch block is in tail position; its binding is released before
// the next round
const viaCatch = function f(n: any, acc: any): any {
  if (n === 0) return acc;
  try {
    throw { n };
  } catch (err: any) {
    return f(n - 1, acc + (err.n % 2));
  }
};
console.log(viaCatch(DEPTH, 0));

// the operand that decides is not the call: no tail call, still right
const notTail = function f(n: any): any {
  if (n === 0) return 0;
  return (f(n - 1) || 0) + 1;
};
console.log(notTail(1000));
