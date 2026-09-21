// §23.1.3.30 step 1 / §23.1.3.34 — `sort` and `toSorted` ask
// IsCallable of the comparator, and §7.2.3 names three shapes. The
// any-lane kernels have answered all three since rotation 682, but
// the checker refused to send these two methods there at all: they
// were the only pair held out of the any-callback allowlist, on the
// ground that an in-place any-lane write back into a typed block was
// unaudited. The kernel had become kind-aware two months before that
// note was written.
//
// The 2+-arg spelling was worse than a refusal. It was ADMITTED and
// kept in the typed lane, where the comparator site knows only
// `Closure | FnSig` — so `xs.sort(p, extra)` panicked the compiler
// while `xs.sort(p)` merely refused. Same program, two arities, two
// different ways of not working.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const desc: any = (a: any, b: any) => b - a;
const proxyCmp: any = new Proxy(desc, {});
const trapped: any = new Proxy(desc, {
  apply: (_t: any, _th: any, a: any) => a[0] - a[1],
});

console.log("proxy comparator ", attempt(() => [3, 1, 2].sort(proxyCmp).join(",")));
console.log("apply trap       ", attempt(() => [3, 1, 2].sort(trapped).join(",")));
console.log("toSorted proxy   ", attempt(() => [3, 1, 2].toSorted(proxyCmp).join(",")));
// The arity that used to reach the compiler's own panic. §23.1.3.30
// takes one argument and ignores the rest.
console.log("trailing arg     ", attempt(() => [3, 1, 2].sort(proxyCmp, 1 as any).join(",")));
console.log("trailing, plain  ", attempt(() => [3, 1, 2].sort(desc, 1 as any).join(",")));

// A class object is callable, so it passes IsCallable and then
// §10.2.1 step 2 refuses the call itself — a TypeError, not a
// compile-time refusal.
class K {}
console.log("class comparator ", attempt(() => [3, 1, 2].sort(K as any).join(",")));
// Not callable at all.
console.log("object comparator", attempt(() => [3, 1, 2].sort({} as any).join(",")));
console.log("number comparator", attempt(() => [3, 1, 2].sort(5 as any).join(",")));

// The comparator's answer is coerced per §23.1.3.30 step 4's
// SortCompare — a string comes back through ToNumber.
const strCmp: any = new Proxy((a: any, b: any) => String(b - a), {});
console.log("string result    ", attempt(() => [3, 1, 2].sort(strCmp).join(",")));

// Controls: the shapes that already worked keep working, and the
// typed comparator keeps its typed lane (no `any` anywhere).
console.log("typed comparator ", [3, 1, 2].sort((a: number, b: number) => a - b).join(","));
console.log("no comparator    ", [3, 1, 2].sort().join(","));
console.log("undefined cmp    ", [3, 1, 2].sort(undefined).join(","));
console.log("toSorted typed   ", [3, 1, 2].toSorted((a: number, b: number) => b - a).join(","));
// sort answers its receiver; toSorted answers a fresh array.
const xs: any = [3, 1, 2];
console.log("sort is in place ", xs.sort(proxyCmp) === xs, xs.join(","));
const ys: any = [3, 1, 2];
console.log("toSorted copies  ", ys.toSorted(proxyCmp) === ys, ys.join(","));
// A string array behind the any lane must keep its element repr.
console.log("string elements  ", attempt(() => ["b", "c", "a"].sort(proxyCmp).join(",")));
console.log("string default   ", ["b", "c", "a"].sort().join(","));
