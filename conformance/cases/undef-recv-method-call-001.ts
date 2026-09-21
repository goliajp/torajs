// §6.2.5.5 GetValue on `r.join` takes the property-reference path,
// which is ToObject(r) — a TypeError (§7.1.18) when r is undefined,
// and one that precedes the argument list (§13.3.6.1 step 3). A
// pointer-shaped slot spells undefined with the generic immortal
// cell, and every typed method arm read it as a live value of its
// own type: the join arm loaded a length off the cell and walked
// it. 31 of the 33 array surfaces died with SIGSEGV that way, in
// both spellings — only `.length` and `r[0]` had ever been armed.
//
// Written without a wrapper on purpose. Neither a closure capture
// nor a parameter carries the fact that a slot may be holding the
// cell (`g(r)` with `g(a: number[])` still dies), so any helper
// that shortens this file would hide the very entry it tests.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);

try {
  console.log("join", "ok", typeof (r.join()));
} catch (e) {
  console.log("join", "threw", (e as Error).constructor.name);
}

try {
  console.log("pop", "ok", typeof (r.pop()));
} catch (e) {
  console.log("pop", "threw", (e as Error).constructor.name);
}

try {
  console.log("push", "ok", typeof (r.push(1)));
} catch (e) {
  console.log("push", "threw", (e as Error).constructor.name);
}

try {
  console.log("shift", "ok", typeof (r.shift()));
} catch (e) {
  console.log("shift", "threw", (e as Error).constructor.name);
}

try {
  console.log("unshift", "ok", typeof (r.unshift(1)));
} catch (e) {
  console.log("unshift", "threw", (e as Error).constructor.name);
}

try {
  console.log("slice", "ok", typeof (r.slice(0)));
} catch (e) {
  console.log("slice", "threw", (e as Error).constructor.name);
}

try {
  console.log("splice", "ok", typeof (r.splice(0)));
} catch (e) {
  console.log("splice", "threw", (e as Error).constructor.name);
}

try {
  console.log("indexOf", "ok", typeof (r.indexOf(1)));
} catch (e) {
  console.log("indexOf", "threw", (e as Error).constructor.name);
}

try {
  console.log("lastIndexOf", "ok", typeof (r.lastIndexOf(1)));
} catch (e) {
  console.log("lastIndexOf", "threw", (e as Error).constructor.name);
}

try {
  console.log("includes", "ok", typeof (r.includes(1)));
} catch (e) {
  console.log("includes", "threw", (e as Error).constructor.name);
}

try {
  console.log("concat", "ok", typeof (r.concat(1)));
} catch (e) {
  console.log("concat", "threw", (e as Error).constructor.name);
}

try {
  console.log("reverse", "ok", typeof (r.reverse()));
} catch (e) {
  console.log("reverse", "threw", (e as Error).constructor.name);
}

try {
  console.log("sort", "ok", typeof (r.sort()));
} catch (e) {
  console.log("sort", "threw", (e as Error).constructor.name);
}

try {
  console.log("fill", "ok", typeof (r.fill(1)));
} catch (e) {
  console.log("fill", "threw", (e as Error).constructor.name);
}

try {
  console.log("flat", "ok", typeof (r.flat()));
} catch (e) {
  console.log("flat", "threw", (e as Error).constructor.name);
}

try {
  console.log("keys", "ok", typeof (r.keys()));
} catch (e) {
  console.log("keys", "threw", (e as Error).constructor.name);
}

try {
  console.log("values", "ok", typeof (r.values()));
} catch (e) {
  console.log("values", "threw", (e as Error).constructor.name);
}

try {
  console.log("entries", "ok", typeof (r.entries()));
} catch (e) {
  console.log("entries", "threw", (e as Error).constructor.name);
}

try {
  console.log("forEach", "ok", typeof (r.forEach((x: number) => x > 0)));
} catch (e) {
  console.log("forEach", "threw", (e as Error).constructor.name);
}

try {
  console.log("map", "ok", typeof (r.map((x: number) => x > 0)));
} catch (e) {
  console.log("map", "threw", (e as Error).constructor.name);
}

try {
  console.log("filter", "ok", typeof (r.filter((x: number) => x > 0)));
} catch (e) {
  console.log("filter", "threw", (e as Error).constructor.name);
}

try {
  console.log("reduce", "ok", typeof (r.reduce((a: number, b: number) => a + b, 0)));
} catch (e) {
  console.log("reduce", "threw", (e as Error).constructor.name);
}

try {
  console.log("every", "ok", typeof (r.every((x: number) => x > 0)));
} catch (e) {
  console.log("every", "threw", (e as Error).constructor.name);
}

try {
  console.log("some", "ok", typeof (r.some((x: number) => x > 0)));
} catch (e) {
  console.log("some", "threw", (e as Error).constructor.name);
}

try {
  console.log("find", "ok", typeof (r.find((x: number) => x > 0)));
} catch (e) {
  console.log("find", "threw", (e as Error).constructor.name);
}

try {
  console.log("findIndex", "ok", typeof (r.findIndex((x: number) => x > 0)));
} catch (e) {
  console.log("findIndex", "threw", (e as Error).constructor.name);
}

try {
  console.log("at", "ok", typeof (r.at(1)));
} catch (e) {
  console.log("at", "threw", (e as Error).constructor.name);
}

try {
  console.log("toString", "ok", typeof (r.toString()));
} catch (e) {
  console.log("toString", "threw", (e as Error).constructor.name);
}

try {
  console.log("copyWithin", "ok", typeof (r.copyWithin(0)));
} catch (e) {
  console.log("copyWithin", "threw", (e as Error).constructor.name);
}

try {
  console.log("toSorted", "ok", typeof (r.toSorted()));
} catch (e) {
  console.log("toSorted", "threw", (e as Error).constructor.name);
}

try {
  console.log("toReversed", "ok", typeof (r.toReversed()));
} catch (e) {
  console.log("toReversed", "threw", (e as Error).constructor.name);
}

try {
  console.log("toSpliced", "ok", typeof (r.toSpliced(0, 0)));
} catch (e) {
  console.log("toSpliced", "threw", (e as Error).constructor.name);
}

try {
  console.log("with", "ok", typeof (r.with(0, 1)));
} catch (e) {
  console.log("with", "threw", (e as Error).constructor.name);
}

try {
  console.log("length", "ok", typeof (r.length));
} catch (e) {
  console.log("length", "threw", (e as Error).constructor.name);
}

try {
  console.log("index", "ok", typeof (r[0]));
} catch (e) {
  console.log("index", "threw", (e as Error).constructor.name);
}

// --- the throw precedes the argument list
let armed = 0;
function loud(): number {
  armed = armed + 1;
  return 1;
}
try {
  console.log("arg-order", "ok", typeof r.push(loud()));
} catch (e) {
  console.log("arg-order", "threw", (e as Error).constructor.name);
}
console.log("args-evaluated", armed);

// --- a live receiver is untouched, and evaluates exactly once
const live = [[1, 2], [3]];
const hit = live.find((z: number[]) => z.length > 1);
console.log(hit.join("-"), hit.length, hit[0], hit.indexOf(2));

let visits = 0;
console.log(
  live
    .find(() => {
      visits = visits + 1;
      return true;
    })
    .join(),
);
console.log("visits", visits);

// --- the other receiver families keep their own answers
const s = "abc";
console.log(s.slice(1), s.indexOf("b"), s.at(0));
const m = new Map<string, number>([["a", 1]]);
console.log(m.get("a"));
class K {
  v = 3;
  m(): number {
    return this.v;
  }
}
console.log(new K().m());

// --- an `any` receiver keeps its own lane, which already answered
const anyUndef: any = undefined;
try {
  console.log("any-lane", "ok", typeof anyUndef.join());
} catch (e) {
  console.log("any-lane", "threw", (e as Error).constructor.name);
}
