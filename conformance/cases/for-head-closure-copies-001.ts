// Closures a `for (let …)` head makes, in plain functions. §14.7.4.4:
// the first copy of the head's bindings is made after the initializer,
// and each later one at the end of an iteration, before the update; the
// test, the body and the update after a copy share it. So a closure the
// update makes sees the increment that follows it (1, 2, 3), one the
// test makes sees its iteration's value, and one the initializer makes
// sees the head's own binding, which no copy changes (0), including
// through a parameter default (§10.2.11 evaluates it at the call, in
// the closure's scope).
function update(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; fs.push(() => i), i++);
  return fs.map((h) => h()).join(",");
}

function updateDefault(): string {
  const fs: ((y?: number) => number)[] = [];
  for (let i = 0; i < 3; fs.push((x: number = i) => x), i++);
  return fs.map((h) => h()).join(",");
}

function updateString(): string {
  const fs: (() => string)[] = [];
  for (let s = "a"; s.length < 4; fs.push(() => s), s = s + "b");
  return fs.map((h) => h()).join(",");
}

function test(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; fs.push(() => i) > 0 && i < 3; i++) {}
  return fs.map((h) => h()).join(",");
}

function init(): string {
  const out: number[] = [];
  let last: () => number = () => -1;
  for (let i = 0, g = () => i; i < 3; i++) {
    out.push(i, g());
    last = g;
  }
  out.push(last());
  return out.join(",");
}

function initWrites(): string {
  const out: number[] = [];
  for (let i = 0, bump = () => ++i; i < 3; i++) out.push(i, bump());
  return out.join(",");
}

function initLaterWrite(): string {
  const out: number[] = [];
  for (let i = 0, g = () => i, h = (i = 5); i < 8; i++) out.push(i, g(), h);
  return out.join(",");
}

function initDefault(): string {
  const out: number[] = [];
  for (let i = 0, g = (x: number = i) => x; i < 3; i++) out.push(g());
  return out.join(",");
}

function initDefaultAny(): string {
  const out: any[] = [];
  for (let i = 0, g = (x = i) => x; i < 3; i++) out.push(g());
  return out.join(",");
}

function withBody(): string {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 4; fs.push(() => i * 10), i++) {
    fs.push(() => i);
    if (i === 1) continue;
    if (i === 2) break;
  }
  return fs.map((h) => h()).join(",");
}

function nested(): string {
  const fs: (() => string)[] = [];
  for (let i = 0; i < 2; i++) {
    for (let j = 0; j < 2; fs.push(() => i + ":" + j), j++);
  }
  return fs.map((h) => h()).join(",");
}

function returnsMidway(): string {
  const fs: (() => number)[] = [];
  const run = (): number => {
    for (let i = 0; i < 5; fs.push(() => i), i++) if (i === 2) return i;
    return -1;
  };
  return run() + " " + fs.map((h) => h()).join(",");
}

// a `var` has no copies (§14.7.4.2): every closure sees the last value
function varHead(): string {
  const fs: (() => number)[] = [];
  for (var i = 0; i < 3; fs.push(() => i), i++) fs.push(() => i * 10);
  return fs.map((h) => h()).join(",");
}

console.log("update", update());
console.log("updateDefault", updateDefault());
console.log("updateString", updateString());
console.log("test", test());
console.log("init", init());
console.log("initWrites", initWrites());
console.log("initLaterWrite", initLaterWrite());
console.log("initDefault", initDefault());
console.log("initDefaultAny", initDefaultAny());
console.log("withBody", withBody());
console.log("nested", nested());
console.log("returnsMidway", returnsMidway());
console.log("varHead", varHead());
