// §7.1.17 ToString takes every value, and a pointer-shaped slot the
// checker typed `T | undefined` can be holding undefined — spelled
// in-band as the generic immortal cell. Every arm of the typed
// ToString read that slot as a live value of its own type:
//
//   Obj     → handed the cell to the ToPrimitive ladder, which looked
//             for `toString` on a cell that has none and threw
//             "value is not a function on this any receiver"
//   Closure → asked the source registry and got
//             "function () { [native code] }"
//
// `const o: { a: number } | undefined = undefined; String(o)` threw,
// in a program every engine answers "undefined" for. The nullish
// pre-check in front of those arms only ever tested NULL, which is
// how `null` is spelled in-band — not how undefined is.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const o: { a: number } | undefined = undefined;
const f: (() => number) | undefined = undefined;
const a: number[] | undefined = undefined;
const s: string | undefined = undefined;
const on: { a: number } | null = null;

console.log("obj undef ", attempt(() => String(o)));
console.log("fn undef  ", attempt(() => String(f)));
console.log("arr undef ", attempt(() => String(a)));
console.log("str undef ", attempt(() => String(s)));
console.log("template  ", attempt(() => `${o}`));

// null keeps its own answer — the two are different values and the
// in-band spellings are different too.
console.log("obj null  ", attempt(() => String(on)));

// An async body that runs off its end settles with undefined, and its
// value slot is the same kind of slot.
async function tail(): Promise<{ a: number }> {}
tail().then((v: any) => console.log("async tail", attempt(() => String(v))));

// Every live value takes the arm it took before — the guard reads the
// checker's nullable typing, so a slot that was never nullable cannot
// reach it.
console.log("live obj  ", attempt(() => {
  const p: { a: number } | undefined = { a: 1 };
  return String(p);
}));
console.log("live hook ", attempt(() => {
  class K {
    toString() {
      return "K!";
    }
  }
  const k: K | undefined = new K();
  return String(k);
}));
console.log("live arr  ", attempt(() => {
  const q: number[] | undefined = [1, 2];
  return String(q);
}));
console.log("live fn   ", attempt(() => {
  const g: (() => number) | undefined = () => 1;
  return typeof String(g);
}));
console.log("plain obj ", attempt(() => String({ a: 1 })));
console.log("plain arr ", attempt(() => String([1, 2])));
