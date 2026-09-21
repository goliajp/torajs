// §7.3.11 GetMethod step 4 is IsCallable, and every symbol-keyed
// protocol hook in the any lane routed through one probe that asked
// instead whether the value held a closure `(env, entry)` pair. The
// other two shapes §7.2.3 names have no pair of their own, so a
// callable Proxy hook was refused at every one of them — with each
// operation's own wrong message: "value is not iterable" for
// @@iterator, "Symbol.toPrimitive is not a function", and so on.
//
// A shape with no pair cannot be given one to jump to, so it gets a
// thunk bound to the receiver the caller was about to invoke with.
// The closure path is first and mints nothing.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const twoStep: any = new Proxy(function () {
  let i = 0;
  return { next: () => (i < 2 ? { value: i++, done: false } : { done: true }) };
}, {});

// §7.1.1 — @@toPrimitive wins over valueOf / toString, and its hint
// argument still arrives: the trap answers it back.
const tp: any = {};
tp[Symbol.toPrimitive] = new Proxy((h: string) => h.length, {});
console.log("toPrimitive  ", attempt(() => tp * 1));

// §7.4.2 GetIterator — the receiver reaches the hook as `this`.
const it: any = {};
it[Symbol.iterator] = twoStep;
console.log("iterator     ", attempt(() => Array.from(it).join(",")));
console.log("spread       ", attempt(() => [...it].join(",")));

// §7.4.9 IteratorClose — a `return` that is a proxy runs on break.
let closed = "no";
const closable: any = {};
closable[Symbol.iterator] = () => ({
  next: () => ({ value: 1, done: false }),
  return: new Proxy(() => {
    closed = "yes";
    return { done: true };
  }, {}),
});
console.log("close hook   ", attempt(() => {
  for (const _ of closable) break;
  return closed;
}));

// §22.1.3 — the String methods that defer to a @@match / @@replace /
// @@search / @@split hook on their argument.
const m: any = {};
m[Symbol.match] = new Proxy((s: string) => "M:" + s, {});
console.log("Symbol.match ", attempt(() => "ab".match(m)));
const r: any = {};
r[Symbol.replace] = new Proxy((s: string, v: any) => s + "/" + v, {});
console.log("Symbol.replce", attempt(() => "ab".replace(r, "X")));
const se: any = {};
se[Symbol.search] = new Proxy(() => 3, {});
console.log("Symbol.search", attempt(() => "ab".search(se)));
const sp: any = {};
sp[Symbol.split] = new Proxy((s: string) => [s, "!"], {});
console.log("Symbol.split ", attempt(() => "ab".split(sp).join("|")));

// Present but not callable keeps step 4's TypeError, per operation.
const bad: any = {};
bad[Symbol.iterator] = 5;
console.log("bad iterator ", attempt(() => [...bad].join(",")));
const badp: any = {};
badp[Symbol.toPrimitive] = 5;
console.log("bad toPrim   ", attempt(() => badp * 1));

// Absent / nullish is NOT the same as present-and-not-callable: the
// caller's own step decides, and for @@toPrimitive that means the
// ordinary valueOf / toString walk still runs.
const nullish: any = { valueOf: () => 8 };
nullish[Symbol.toPrimitive] = undefined;
console.log("nullish hook ", attempt(() => nullish * 1));

// The ordinary closure path, one branch unchanged.
const plain: any = {};
plain[Symbol.iterator] = function () {
  let i = 0;
  return { next: () => (i < 2 ? { value: i++ * 10, done: false } : { done: true }) };
};
console.log("closure iter ", attempt(() => [...plain].join(",")));
