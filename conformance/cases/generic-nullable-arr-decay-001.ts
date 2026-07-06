// RC-4 F1a companion — a Nullable<Array<Str>> arg (exec/match
// result) decays to its inner Array against an Array-shaped generic
// param (the test262 harness's compareArray<T>(actual: T[], ...)
// shape rejected exec results after the chunk-598 retype). The
// runtime null stays guarded: a miss result entering the
// monomorphized callee arms a catchable TypeError instead of a
// silent null deref.

function count<T>(xs: T[]): number {
  return xs.length;
}

function firstOf<T>(xs: T[], fallback: T): T {
  return xs.length > 0 ? xs[0] : fallback;
}

// hit: exec result flows into generic Array params
const m = /b(c)/.exec("abcd");
console.log(count(m));
console.log(firstOf(m, "none"));

// narrowed use keeps working alongside the decay
if (m !== null) {
  console.log(m.length);
}

// miss: the decayed null arms a TypeError inside the callee
const miss = /zz/.exec("abcd");
try {
  console.log(count(miss));
} catch (e) {
  console.log("caught");
}
console.log("done");
