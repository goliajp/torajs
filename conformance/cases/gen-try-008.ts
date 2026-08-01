// yield INSIDE finally (for-of yield-from-finally shape) + empty try
let i = 0;
let j = 0;
function* g(): Generator<any> {
  let n: number = 0;
  while (n < 2) {
    try {
    } finally {
      i = i + 1;
      yield i;
      j = j + 1;
    }
    n = n + 1;
  }
  yield "end";
}
const it = g();
console.log(it.next().value);
console.log(j);
console.log(it.next().value);
console.log(j);
console.log(it.next().value);
console.log(j);
console.log(it.next().done);
