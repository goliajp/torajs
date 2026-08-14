// §15.7.14 evaluates each ComputedPropertyName once, at class
// definition — not per construction, and not lazily on first use.
function evaluatedOnce(): number {
  let n = 0;
  const kk = (): string => {
    n++;
    return "z";
  };
  class K {
    [kk()] = 1;
  }
  new K();
  new K();
  new K();
  return n;
}

// The key evaluates where the class is written, so a throw out of it
// is a throw out of the class definition — before any initializer.
function abruptKey(): string {
  const seen: string[] = [];
  const boom = (): string => {
    seen.push("key");
    throw new Error("k");
  };
  try {
    class K {
      [boom()]() {
        return 1;
      }
    }
  } catch (e) {
    seen.push("caught");
  }
  return seen.join(",");
}

// A numeric key, and a computed name that lands on a member the class
// also declares outright — the later declaration wins.
function numericAndShadow(a: number): string {
  const n = 2;
  const k = "m";
  class K {
    [n]() {
      return 7;
    }
    m() {
      return 1;
    }
    [k]() {
      return a;
    }
  }
  const o = new K() as any;
  return o[2]() + "," + o.m();
}

// A Symbol key reaches the same lane.
function symbolKey(a: number): number {
  const s = Symbol("tag");
  class K {
    [s]() {
      return a;
    }
  }
  return (new K() as any)[s]();
}

console.log(evaluatedOnce());
console.log(abruptKey());
console.log(numericAndShadow(5));
console.log(symbolKey(9));
