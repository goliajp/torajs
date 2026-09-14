// §14.7.5.7 step 6 — IteratorClose runs on the completion the loop
// BODY produced; a try statement's completion is its finally's. So a
// `return` inside `try { } catch { } finally { }` inside a for-of runs
// the finally first and closes the iterator after it, outside the
// try's own handlers: a throwing `return()` must not be caught by that
// catch (the loop went on forever when it was).
function mk(throwOnReturn: boolean) {
  const log: string[] = [];
  const it: any = {
    [Symbol.iterator]() {
      return {
        next() { return { done: false, value: 1 }; },
        return() { log.push("close"); if (throwOnReturn) throw 42; return {}; },
      };
    },
  };
  return { it, log };
}
function retInTry(throwOnReturn: boolean) {
  const { it, log } = mk(throwOnReturn);
  try {
    for (const x of it) {
      try { log.push("body"); return "r"; }
      catch (e) { log.push("catch " + e); }
      finally { log.push("finally"); }
    }
    return "end";
  } catch (e) {
    log.push("outer " + e);
    return "outer";
  } finally {
    console.log(log.join(","));
  }
}
console.log(retInTry(true));
console.log(retInTry(false));
function nested(throwOnReturn: boolean) {
  const { it, log } = mk(throwOnReturn);
  const inner = mk(false);
  try {
    for (const a of it) {
      try {
        for (const b of inner.it) {
          try { return "deep"; } finally { log.push("f1"); }
        }
      } finally { log.push("f2"); }
    }
    return "end";
  } catch (e) {
    return "outer " + e;
  } finally {
    console.log(log.join(","), inner.log.join(","));
  }
}
console.log(nested(true));
console.log(nested(false));
