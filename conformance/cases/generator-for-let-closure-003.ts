// Closures made in a loop head read the iteration environment that
// head runs in (§14.7.4.2): a test or step closure the current
// iteration's, a head-declaration closure the head's own, which a write
// in the first iteration no longer reaches.

function* g1() {
  const fs: Array<() => number> = [];
  for (let i = 0, f = () => i; i < 6; i++) {
    fs.push(f);
    if (i === 0) i = 2;
    fs.push(() => i);
    yield i;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g1()].join(" "));

function* g2() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; fs.push(() => i), i++) {
    yield i;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g2()].join(" "));

function* g3() {
  const fs: Array<() => number> = [];
  for (let i = 0; fs.push(() => i) > 0 && i < 3; i++) {
    yield i;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g3()].join(" "));

// two bindings
function* g4() {
  const fs: Array<() => string> = [];
  for (let i = 0, j = 10; i < 3; i++, j--) {
    fs.push(() => i + ":" + j);
    yield j;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g4()].join(" "));

// a step that makes a closure and yields; `continue` still runs it
function* g5() {
  const fs: Array<() => number> = [];
  outer: for (let i = 0; i < 5; fs.push(() => i), i++, yield -1) {
    if (i % 2 === 0) continue outer;
    yield i;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g5()].join(" "));

// closures in the heads of nested loops
function* g6() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    inner: for (let j = 0, f = () => i * 100; j < 2; j++, fs.push(() => i + j)) {
      if (j === 0) continue inner;
      fs.push(f);
    }
    let n = 0;
    while (fs.push(() => i * 10) && n < 1) n++;
    do n++;
    while (fs.push(() => -i) < 0);
    yield i;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g6()].join(" "));
