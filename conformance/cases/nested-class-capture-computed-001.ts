// A computed member name is not in any class body — the parser parks
// the key expression in a side table. The nested-class hoist walked
// only the bodies, so this read as capture-free, lifted to the top
// level, and `k` no longer resolved there.
function methodKey(a: number): number {
  const k = "z";
  class K {
    [k]() {
      return a;
    }
  }
  return (new K() as any)[k]();
}

// A computed INSTANCE field: the key evaluates once at class
// definition, the initializer once per construction.
function fieldKey(a: number): number {
  const k = "z";
  class K {
    [k] = a * 2;
  }
  return (new K() as any)[k];
}

// The key expression sees the enclosing scope it was written in, and
// evaluates in element order.
function keyOrder(): string {
  const seen: string[] = [];
  const kk = (t: string): string => {
    seen.push(t);
    return t;
  };
  class K {
    [kk("a")] = 1;
    [kk("b")]() {
      return 2;
    }
  }
  const o = new K() as any;
  return seen.join(",") + "|" + o.a + "|" + o.b();
}

console.log(methodKey(5));
console.log(fieldKey(3));
console.log(keyOrder());
