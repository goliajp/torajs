// `new Map(<array literal>)` was refused at type-check whenever the
// literal was not a list of two-element array literals — including a
// perfectly good one wearing an `as any`, which is how most real
// spellings arrive. §24.1.1.1 has no such rule: step 8 walks the
// iterable and reads `Get(entry, "0")` / `Get(entry, "1")`, and a
// non-entry is a run-time TypeError, not a compile-time one.
//
// The refusal was load-bearing, which is the part worth writing down:
// underneath it, a SHORT entry read slot 1 past the end of the
// entry's slot buffer and boxed whatever the allocator had left
// there as a heap pointer. `new Map([["a"]])` was a SIGSEGV, and the
// gate that would have caught it was the refusal itself.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
// A short entry: `Get(entry, "1")` is undefined, not a wild read.
console.log("short entry ", attempt(() => {
  const m: any = new Map([["a"]] as any);
  return m.size + ":" + String(m.get("a"));
}));
console.log("empty entry ", attempt(() => new Map([[]] as any).size));
console.log("long entry  ", attempt(() => new Map([["a", 1, 9]] as any).get("a")));

// The shapes the refusal used to swallow whole.
console.log("cast pairs  ", attempt(() => new Map([["a", 1]] as any).get("a")));
console.log("cast mixed  ", attempt(() => new Map([["a", 1], ["b", "x"]] as any).get("b")));
console.log("cast spread ", attempt(() => new Map([...[["a", 1]]] as any).get("a")));
console.log("set cast    ", attempt(() => new Set([1, 2, 2] as any).size));
console.log("set mixed   ", attempt(() => new Set([1, [2, 3]] as any).size));

// A non-entry is §24.1.1.1 step 8.a's run-time TypeError.
console.log("non-entry   ", attempt(() => new Map([1, 2] as any).size));
console.log("string arg  ", attempt(() => new Map("ab" as any).size));
console.log("set string  ", attempt(() => new Set("ab" as any).size));

// Nullish adds nothing (§24.1.1.1 step 6).
console.log("null arg    ", attempt(() => new Map(null as any).size));
console.log("undef arg   ", attempt(() => new Map(undefined as any).size));

// The typed lanes, one branch unchanged.
const pairs: [string, number][] = [["c", 3], ["d", 4]];
console.log("typed arr   ", attempt(() => new Map(pairs).get("d")));
console.log("literal     ", attempt(() => new Map([["a", 1], ["b", 2]]).size));
console.log("copy ctor   ", attempt(() => new Map(new Map([["a", 1]])).get("a")));
console.log("set literal ", attempt(() => new Set([1, 2, 2]).size));
console.log("empty map   ", attempt(() => new Map().size));
