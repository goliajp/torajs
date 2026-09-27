// A `continue` inside a `switch` continues the loop around the switch.
// It used to leave only the switch, so the rest of the loop body ran;
// through a `try … finally`, and inside a generator or async function
// whose loop suspends, it did the same.
function forLoop(): string {
  let out = "";
  for (let i = 0; i < 5; i++) {
    switch (i % 2) {
      case 0: out += "e"; continue;
      default: out += "o";
    }
    out += ".";
  }
  return out;
}

function whileLoop(): string {
  let w = 0;
  let out = "";
  while (w < 5) {
    w++;
    switch (w) {
      case 2:
      case 4: continue;
    }
    out += w;
  }
  return out;
}

function doWhileFinally(): string {
  let i = 0;
  let out = "";
  do {
    i++;
    try {
      switch (i) {
        case 2: continue;
      }
      out += i;
    } finally {
      out += "f";
    }
    out += "|";
  } while (i < 3);
  return out;
}

function forOfNested(): string {
  let out = "";
  for (const s of ["a", "b", "c"]) {
    for (let k = 0; k < 2; k++) {
      switch (s) {
        case "b": continue;
      }
      out += s + k;
    }
  }
  return out;
}

function* gen(n: number): Generator<number> {
  for (let i = 0; i < n; i++) {
    switch (i % 3) {
      case 1: continue;
    }
    yield i;
  }
}

async function run(): Promise<string> {
  let out = "";
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    switch (i) {
      case 2: continue;
    }
    out += i;
  }
  return out;
}

console.log(forLoop(), whileLoop(), doWhileFinally(), forOfNested());
console.log([...gen(7)].join(","));
run().then((s) => console.log(s));
