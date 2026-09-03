// The any-lane argv packer has to know whether the argument it is
// boxing is a fresh temp or a borrow of something a binding already
// owns: a borrow's box takes its own +1, and only a fresh temp's
// reference may be released after the call. Asking that by AST
// shape — `Ident | Member | Regex` — misses every shape wearing a
// cast, and `x as any` is precisely how an any-lane call site is
// written. The release then freed the binding's only stake, and
// whatever allocated next moved into those bytes.
//
// Every block below is its own program's worth of the bug: it needs
// only a borrow-shaped argument under a cast, a call that takes the
// any argv, and an allocation afterwards to move into the hole.

function churn(): number {
  const held: any[] = [];
  for (let i = 0; i < 8; i++) {
    held.push({ a: i, b: i + 1 });
  }
  return held.length;
}

const buf = new ArrayBuffer(4, { maxByteLength: 8 });

// `Ident as any` — a cast around the one shape the roster did list.
const ident = { tag: "ident" };
buf.resize(ident as any);
console.log(churn(), JSON.stringify(ident));

// `Member as any`.
const holder = { inner: { tag: "member" } };
buf.resize(holder.inner as any);
console.log(churn(), JSON.stringify(holder.inner));

// `Index as any` — a shape the roster never listed, cast or not.
const cells = [{ tag: "index" }];
buf.resize(cells[0] as any);
console.log(churn(), JSON.stringify(cells[0]));

// A bare any-call packs the same argv.
const call: any = (x: any) => x.tag;
const bare = { tag: "bare" };
console.log(call(bare as any), churn(), JSON.stringify(bare));

// The face that named it. §25.1.6.6 checks `resizable` BEFORE
// coercing, so the first call throws without ever touching its
// argument — but the packer had already released it, and the error
// the throw allocates lands on the freed object. The second call
// then coerces bytes that belong to someone else: it answered
// `0 0` (a silent wrong length and a `valueOf` that never ran),
// and `String()`ing the object in between answered the array a
// preceding `Object.keys` had built in the same hole.
let coerced = 0;
const counting = {
  valueOf() {
    coerced = coerced + 1;
    return 2;
  },
};
const fixed = new ArrayBuffer(4);
try {
  fixed.resize(counting as any);
} catch (e) {
  console.log((e as Error).constructor.name);
}
const grow = new ArrayBuffer(4, { maxByteLength: 16 });
grow.resize(counting as any);
console.log(grow.byteLength, coerced);
