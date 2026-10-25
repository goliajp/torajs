// A closure a `for` head's initializer makes may name a lexical binding
// the same head declares after it (§14.7.4.2 creates every one of them
// before the initializer runs). Called once the declarations have run,
// it reads the head's own binding: the value it was given, never the
// per-iteration copies.
function constLater(): string {
  const out: number[] = [];
  for (const g = () => k, k = 5; ; ) {
    out.push(g());
    break;
  }
  return out.join(",");
}

function letLater(): string {
  const out: number[] = [];
  for (let g = () => k, k = 0; k < 3; k++) out.push(g(), k);
  return out.join(",");
}

function constString(): string {
  const out: string[] = [];
  for (const g = () => s, s = "x" + out.length; ; ) {
    out.push(g());
    break;
  }
  return out.join(",");
}

function mutual(): string {
  const out: number[] = [];
  for (let i = 0, f = (n: number): number => (n > 0 ? g(n - 1) : 0), g = (n: number): number => f(n) + 1; i < 2; i++)
    out.push(f(i + 2));
  return out.join(",");
}

function varLater(): string {
  const out: number[] = [];
  const fs: (() => number)[] = [];
  for (var g = () => k, k = 0; k < 3; k++) {
    fs.push(() => k);
    out.push(g());
  }
  for (const f of fs) out.push(f());
  return out.join(",");
}

// called before the later declaration has run, it reads a binding in
// its dead zone (§9.1.1.1.6 GetBindingValue step 2)
function deadZone(): string {
  const out: string[] = [];
  try {
    for (const g = () => k, x = g(), k = 1; ; ) {
      out.push("no throw " + x + k);
      break;
    }
  } catch (e: any) {
    out.push(e.constructor.name);
  }
  try {
    for (let i = 0, g = () => j * 2, y = i < 1 ? g() : 0, j = 3; i < 2; i++) out.push("no throw " + y);
  } catch (e: any) {
    out.push(e.constructor.name);
  }
  return out.join(",");
}

const top: number[] = [];
for (const h = () => m * 2, m = 21; ; ) {
  top.push(h());
  break;
}

console.log("constLater", constLater());
console.log("letLater", letLater());
console.log("constString", constString());
console.log("mutual", mutual());
console.log("varLater", varLater());
console.log("deadZone", deadZone());
console.log("top", top.join(","));
