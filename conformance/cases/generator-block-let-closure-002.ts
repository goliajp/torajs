// A block binding a closure reads keeps its pass, and its initializer
// may read a block const declared before a yield of the same pass.

function* num() {
  let n = 0;
  const fs: (() => number)[] = [];
  while (n < 2) {
    const v = n;
    yield 1;
    let u = v + 7;
    fs.push(() => u);
    u += 100;
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of num()) {}

function* str() {
  let n = 0;
  const fs: (() => string)[] = [];
  while (n < 2) {
    const s = "k" + n;
    yield 1;
    let u = s + "!";
    fs.push(() => u);
    u += "?";
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of str()) {}

function* obj() {
  let n = 0;
  const fs: (() => number)[] = [];
  while (n < 2) {
    const o = { x: n * 3 };
    yield 1;
    let u = o.x + 7;
    fs.push(() => u + o.x);
    u += 100;
    o.x += 1;
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of obj()) {}

function* twoYields() {
  let n = 0;
  const fs: (() => number)[] = [];
  while (n < 2) {
    const v = n;
    yield 1;
    yield 2;
    let u = v + 7;
    fs.push(() => u);
    u += 100;
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of twoYields()) {}

function* nested() {
  let n = 0;
  const fs: (() => number)[] = [];
  while (n < 2) {
    const v = n;
    {
      yield 1;
      let u = v + 7;
      fs.push(() => u);
      {
        const w = u + v;
        yield 2;
        fs.push(() => w);
      }
      u += 100;
    }
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of nested()) {}

function* unboxed() {
  let n = 0;
  const fs: (() => number)[] = [];
  while (n < 2) {
    const v = n;
    yield 1;
    const u = v + 7;
    fs.push(() => u + v);
    n++;
  }
  for (const f of fs) console.log(f());
}
for (const _ of unboxed()) {}
