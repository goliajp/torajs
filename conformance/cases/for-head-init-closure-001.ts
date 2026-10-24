// A `for` whose initializer makes a function that reads none of the
// head's `let` bindings: each iteration still gets its own copy of
// them (§14.7.4.4 CreatePerIterationEnvironment), so the closures the
// body makes see 0, 1, 2. A `const` in the head has no copies
// (§14.7.4.2), and a function over it changes nothing.
function plain(): string {
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => 0; i < 3; i++) fs.push(() => i);
  return fs.map((h) => h()).join(",");
}

function callsInitFn(): string {
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => 10; i < 3; i++) fs.push(() => i + g());
  return fs.map((h) => h()).join(",");
}

function mutatesInBody(): string {
  const fs: (() => string)[] = [];
  for (let i = 0, s = "a", g = (x: string) => x + "!"; i < 3; i++) {
    s = s + i;
    fs.push(() => g(s) + i);
  }
  return fs.map((h) => h()).join(",");
}

function constHead(): string {
  const fs: (() => number)[] = [];
  for (const k = 5, g = () => k; fs.length < 3; ) fs.push(() => k + g());
  return fs.map((h) => h()).join(",");
}

console.log(plain());
console.log(callsInitFn());
console.log(mutatesInBody());
console.log(constHead());
