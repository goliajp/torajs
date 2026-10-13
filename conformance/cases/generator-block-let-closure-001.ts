// a let / const declared in a block that runs once per loop pass keeps that pass's value in the closures made after it, in a generator
function* forBody() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    let n = i * 10;
    yield n;
    fs.push(() => n);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...forBody()].join(" "));
function* doWhileBody() {
  const fs: Array<() => number> = [];
  let i = 0;
  do {
    const n = i * 2;
    yield n;
    fs.push(() => n);
    i++;
  } while (i < 3);
  yield fs.map((f) => f()).join(",");
}
console.log([...doWhileBody()].join(" "));
function* forOfBody() {
  const fs: Array<() => string> = [];
  for (const s of ["a", "b", "c"]) {
    const t = s + s;
    yield t;
    fs.push(() => t);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...forOfBody()].join(" "));
function* nestedBlock() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    {
      let n = i + 100;
      yield;
      fs.push(() => n);
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...nestedBlock()].join(" "));
function* written() {
  const fs: Array<() => number> = [];
  const ws: Array<(v: number) => void> = [];
  for (let i = 0; i < 3; i++) {
    let n = i;
    yield n;
    fs.push(() => n);
    ws.push((v: number) => {
      n = v;
    });
    n += 5;
  }
  ws[1](42);
  yield fs.map((f) => f()).join(",");
}
console.log([...written()].join(" "));
function* noSuspend() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    let n = i;
    fs.push(() => n);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...noSuspend()].join(" "));
function* resumeValue() {
  const fs: Array<() => any> = [];
  for (let i = 0; i < 3; i++) {
    const n = yield i;
    fs.push(() => n);
  }
  yield fs.map((f) => f()).join(",");
}
const it = resumeValue();
const out: any[] = [it.next().value];
out.push(it.next("x").value);
out.push(it.next("y").value);
out.push(it.next("z").value);
console.log(out.join(" "));
function* labelledContinue() {
  const fs: Array<() => number> = [];
  outer: for (let i = 0; i < 4; i++) {
    const n = i * 3;
    const f = () => n;
    fs.push(f);
    if (i === 1) continue outer;
    yield n;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...labelledContinue()].join(" "));
function* nestedLoopTest() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 2; i++) {
    const n = i;
    for (let j = 0; j < 2 && fs.push(() => n + j * 10) > 0; j++) {
      yield j;
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...nestedLoopTest()].join(" "));
function* switchCase() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    switch (i) {
      case 0: {
        const n = 7;
        yield n;
        fs.push(() => n);
        break;
      }
      default: {
        let n = i * 11;
        yield n;
        fs.push(() => n);
      }
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...switchCase()].join(" "));
function* tryBody() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    try {
      const n = i * 4;
      yield n;
      fs.push(() => n);
    } finally {
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...tryBody()].join(" "));
function* twoDeclarators() {
  const fs: Array<() => number> = [];
  let i = 0;
  while (i < 3) {
    let a = i, b = i * 2;
    yield;
    fs.push(() => a + b);
    i++;
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...twoDeclarators()].join(" "));
