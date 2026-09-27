// A generator body's do-while loop may yield: the body runs before
// the test, `continue` still evaluates the test, and a labeled
// `continue` from an inner loop reaches it. It used to be rejected
// with "yield is only valid inside a `function*` generator body".
function* countdown(n: number) {
  do {
    yield n;
    n--;
  } while (n > 0);
}
function* skipOdd(n: number) {
  let i = 0;
  do {
    i++;
    if (i % 2 === 1) continue;
    yield i;
  } while (i < n);
}
function* once() {
  let ran = 0;
  do {
    ran++;
    yield ran;
  } while (false);
  yield -1;
}
function* labeled() {
  let i = 0;
  outer: do {
    i++;
    let j = 0;
    while (j < 3) {
      j++;
      if (j === 2) continue outer;
      yield i * 10 + j;
    }
  } while (i < 3);
}
function* early() {
  let k = 0;
  do {
    k++;
    if (k === 3) break;
    yield k;
  } while (true);
  yield 99;
}
const log: string[] = [];
function* guarded() {
  let i = 0;
  try {
    do {
      i++;
      yield i;
      if (i === 2) return;
    } while (true);
  } finally {
    log.push("fin" + i);
  }
}
console.log([...countdown(3)].join(","));
console.log([...skipOdd(6)].join(","));
console.log([...once()].join(","));
console.log([...labeled()].join(","));
console.log([...early()].join(","));
console.log([...guarded()].join(","), log.join(","));
