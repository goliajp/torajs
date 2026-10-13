// The generator shapes of a per-iteration `for (let …)` binding, in an
// async function body: `await` suspends where `yield` did.

async function a1() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++, await 0) {
    fs.push(() => i);
  }
  console.log("a1", fs.map((f) => f()).join(","));
}

async function a2() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    fs.push((await 0, () => i));
  }
  console.log("a2", fs.map((f) => f()).join(","));
}

async function a3() {
  const fs: Array<() => number> = [];
  for (let i = 0; (await 0, i < 3); i++) {
    fs.push(() => i);
  }
  console.log("a3", fs.map((f) => f()).join(","));
}

async function a4() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; fs.push(() => i), i++) {
    await 0;
  }
  console.log("a4", fs.map((f) => f()).join(","));
}

async function a5() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    await fs.push(() => i);
  }
  console.log("a5", fs.map((f) => f()).join(","));
}

async function a6() {
  const fs: Array<() => number> = [];
  for (let i = 0, f = () => i; i < 3; i++) {
    fs.push(f);
    fs.push(() => i);
    await 0;
  }
  console.log("a6", fs.map((f) => f()).join(","));
}

async function a7() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    if (fs.push(() => i) > 0) await 0;
  }
  console.log("a7", fs.map((f) => f()).join(","));
}

a1().then(a2).then(a3).then(a4).then(a5).then(a6).then(a7);
