// The ways out of a `for (let …)` loop whose test or update makes
// closures over the head's bindings (§14.7.4.4): a labelled continue of
// the outer loop, a throw caught outside, and a non-Copy binding left
// by break or return. Each closure keeps the copy of the iteration
// that made it.
function labelled(): string {
  const fs: (() => number)[] = [];
  const gs: (() => number)[] = [];
  outer: for (let i = 0; i < 3; fs.push(() => i), i++)
    for (let j = 0; j < 3; gs.push(() => j), j++) {
      if (j === 1) continue outer;
    }
  return fs.map((h) => h()).join(",") + " " + gs.map((h) => h()).join(",");
}

function labelledBreak(): string {
  const fs: (() => number)[] = [];
  outer: for (let i = 0; i < 5; fs.push(() => i), i++)
    for (let j = 0; j < 3; fs.push(() => i * 10 + j), j++) {
      if (i === 2) break outer;
    }
  return fs.map((h) => h()).join(",");
}

function thrown(): string {
  const fs: (() => number)[] = [];
  try {
    for (let i = 0; i < 5; fs.push(() => i), i++) {
      if (i === 3) throw new Error("at " + i);
    }
  } catch (e: any) {
    fs.push(() => -1);
    return e.message + " " + fs.map((h) => h()).join(",");
  }
  return "none";
}

function thrownByTest(): string {
  const fs: (() => number)[] = [];
  const check = (i: number): boolean => {
    if (i === 2) throw new Error("test " + i);
    return i < 5;
  };
  try {
    for (let i = 0; check(i); fs.push(() => i), i++);
  } catch (e: any) {
    return e.message + " " + fs.map((h) => h()).join(",");
  }
  return "none";
}

function objBreak(): string {
  const fs: (() => number)[] = [];
  for (let o = { n: 0 }; o.n < 3; fs.push(() => o.n), o = { n: o.n + 1 }) if (o.n === 1) break;
  return fs.map((h) => h()).join(",");
}

function objReturn(): string {
  const fs: (() => number)[] = [];
  const run = (): number => {
    for (let o = { n: 0 }; o.n < 5; fs.push(() => o.n), o = { n: o.n + 1 }) if (o.n === 2) return o.n;
    return -1;
  };
  return run() + " " + fs.map((h) => h()).join(",");
}

function strReturn(p: string): string {
  const fs: (() => string)[] = [];
  const run = (): (() => string)[] => {
    for (let s = p + "a"; s.length < 9; fs.push(() => s), s = s + "b") if (s.length === 3) return fs;
    return [];
  };
  return run().map((h) => h()).join(",");
}

console.log("labelled", labelled());
console.log("labelledBreak", labelledBreak());
console.log("thrown", thrown());
console.log("thrownByTest", thrownByTest());
console.log("objBreak", objBreak());
console.log("objReturn", objReturn());
console.log("strReturn", strReturn("x"));
