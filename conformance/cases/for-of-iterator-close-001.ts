// RFC 20260901-scope-exit-drops 刀 2 — a for-of that stops early
// closes its iterator (§7.4.9 IteratorClose) on EVERY abrupt exit, not
// only on `break` / `return`: a throw out of the body calls `return()`
// with the original throw suspended (the original wins, the close's
// own throw is discarded), a labeled jump over inner loops closes each
// of them innermost-first. The iterator slot itself is a scoped local
// now, so the churn probe sees a flat RSS where the MapIter used to
// leak. (A generator's `finally` cannot observe the close yet — the
// close call does not reach a generator's `return()` — so the
// witnesses here are `return()` methods that log.)
const boom = (): any => {
  throw new Error("boom");
};
const log: string[] = [];

function iter(tag: string, limit: number, throwOnClose: boolean): any {
  let i = 0;
  return {
    next() {
      i = i + 1;
      return { value: i, done: i > limit };
    },
    return() {
      log.push("closed:" + tag);
      if (throwOnClose) throw new Error("from-close");
      return { value: 0, done: true };
    },
  };
}
const seq = (tag: string, throwOnClose: boolean): any => ({
  [Symbol.iterator]() {
    return iter(tag, 3, throwOnClose);
  },
});

// 1. throw out of the body → return() runs, the original throw
//    reaches the catch
try {
  for (const v of seq("throw", false)) {
    if ((v as number) === 2) boom();
    log.push("v" + String(v));
  }
} catch (e) {
  log.push("caught:" + (e as Error).message);
}

// 2. `throw` statement inside the body; the close itself throws and
//    is discarded — the original wins
try {
  for (const v of seq("stmt", true)) {
    if ((v as number) === 1) throw new Error("stmt");
  }
} catch (e) {
  log.push("caught:" + (e as Error).message);
}

// 3. labeled continue / break over an inner iterator loop
outer: for (let i = 0; i < 2; i++) {
  for (const v of seq("inner" + i, false)) {
    if ((v as number) === 2) continue outer;
    log.push("i" + i + "v" + String(v));
  }
}
lab: for (let i = 0; i < 1; i++) {
  for (const v of seq("brk", false)) {
    if ((v as number) === 1) break lab;
  }
}

// 4. return out of two nested for-ofs — closes both, innermost first
const twice = (): number => {
  for (const a of seq("ret-outer", false)) {
    for (const b of seq("ret-inner", false)) {
      if ((b as number) === 2) return (a as number) * 10 + (b as number);
    }
  }
  return -1;
};
log.push("ret:" + twice());

// 5. Map / Set for-of whose body throws (the MapIter case the churn
//    probe caught), and a plain break for the control shape
const m = new Map<number, string>([[1, "a"], [2, "b"]]);
try {
  for (const [k, v] of m) {
    if (k === 2) boom();
    log.push("m" + k + v);
  }
} catch (e) {
  log.push("caught:map");
}
const st = new Set<string>(["x", "y"]);
for (const v of st) {
  if (v === "y") break;
  log.push("s" + v);
}

// 6. natural completion does not close; a `continue` keeps stepping
let sum = 0;
for (const v of seq("full", false)) {
  if ((v as number) === 2) continue;
  sum += v as number;
}
log.push("sum:" + sum);

// 7. throw out of a nested pair — both close, innermost first
try {
  for (const a of seq("nest-outer", false)) {
    for (const b of seq("nest-inner", false)) {
      if ((b as number) === 1) boom();
    }
  }
} catch (e) {
  log.push("caught:nest");
}

console.log(log.join(" "));
