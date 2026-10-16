// `(o?.m)(...xs)` with a nullish `o`: the callee is `undefined`, but
// the argument list still evaluates first (§13.3.8.1), so every
// spread source is iterated to the end before the TypeError.

const log: string[] = [];
function* gen(n: number) {
  for (let i = 0; i < n; i++) {
    log.push("next" + i);
    yield i;
  }
  log.push("done");
}
const iterable = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        log.push("it" + i);
        return i < 2 ? { value: i++, done: false } : { value: undefined, done: true };
      },
    };
  },
};
function tag(v: string): string {
  log.push(v);
  return v;
}

function call(o: any, xs: number[]): string {
  try {
    return String((o?.m)(tag("a"), ...xs, ...gen(2), ...iterable, tag("z")));
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

const n = process.argv.length > 99 ? 0 : 1;
const present = {
  k: 100,
  m(...args: any[]) {
    return this.k + args.length;
  },
};
console.log(call(n > 0 ? present : null, [7, 8]));
console.log(log.join(","));
log.length = 0;
for (const o of [null, undefined]) {
  console.log(call(o, [1, 2, 3]));
  console.log(log.join(","));
  log.length = 0;
}
try {
  const nul: any = n > 0 ? null : present;
  const bad: any = n > 0 ? null : [];
  (nul?.m)(...gen(1), ...bad);
} catch (e) {
  console.log("not iterable:", e instanceof TypeError, log.join(","));
}
