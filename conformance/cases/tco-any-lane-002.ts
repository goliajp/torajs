// Calls through an `any` callee that are not in tail position
// (sec-isintailposition): inside a try block, under a finally, in an
// async function, in a generator, and as an operand. Each keeps its
// frame across the call; the depth stays small and the results must
// match.
let h: any = function (n: any): any {
  if (n === 0) throw new Error("zero");
  return n * 2;
};

function inTry(n: any): any {
  try {
    return h(n);
  } catch (e: any) {
    return "caught " + e.message;
  }
}
console.log(inTry(3), inTry(0));

let finallyRuns = 0;
function underFinally(n: any): any {
  try {
    return h(n);
  } finally {
    finallyRuns += 1;
  }
}
console.log(underFinally(4), finallyRuns);

async function inAsync(n: any): Promise<any> {
  return h(n);
}

function* inGenerator(n: any): any {
  yield n;
  return h(n);
}
const it = inGenerator(5);
console.log(it.next().value, it.next().value);

function operand(n: any): any {
  return h(n) + 1;
}
console.log(operand(6));

let g: any;
g = function (n: any): any {
  if (n === 0) return 0;
  return 1 + g(n - 1);
};
console.log(g(1000));

inAsync(7).then((v: any) => console.log("async", v));
