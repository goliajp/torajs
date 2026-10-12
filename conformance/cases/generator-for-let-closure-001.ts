// A `for (let …)` loop gets a fresh copy of its binding for every
// iteration (§14.7.4.2 CreatePerIterationEnvironment), so a closure
// made in one iteration keeps that iteration's value.
function* g1() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) fs.push(() => i);
  for (const f of fs) yield f();
}
for (const v of g1()) console.log(v);

function* g2() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    fs.push(() => i);
    yield i;
  }
  for (const f of fs) yield f();
}
for (const v of g2()) console.log(v);

// a write after the closure is made is seen by that closure; the copy
// for the next iteration starts from the written value
function* g3() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 6; i++) {
    fs.push(() => i);
    yield i;
    i++;
  }
  for (const f of fs) yield f();
}
for (const v of g3()) console.log(v);

function* g4() {
  const ss: (() => string)[] = [];
  for (let i = 0; i < 3; i++) {
    if (i === 1) continue;
    ss.push(function () {
      return "f" + i;
    });
  }
  for (const s of ss) yield s();
}
for (const v of g4()) console.log(v);

async function a1() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) fs.push(() => i);
  await 0;
  for (const f of fs) console.log("a1", f());
}

async function a2() {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    fs.push(() => i * 10);
    await 0;
  }
  for (const f of fs) console.log("a2", f());
}
a1().then(a2);
