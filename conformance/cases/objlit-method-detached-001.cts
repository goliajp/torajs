// RFC 20260813-detached-objlit-method — an object-literal method read
// as a VALUE. The nominal shape gives the body a `__this: __ObjLit_n`
// param it dereferences at struct offsets, but a call through the
// detached value lowers to the env-first CallIndirect, which has no
// receiver slot at all: every line below SIGSEGVed before the widen.
// `.cts` so the script is sloppy — a bare call's receiver is the
// global object, hence `undefined` for `this.n` rather than a throw.
const o = {
  n: 7,
  read() {
    return this.n;
  },
  bump() {
    this.n = this.n + 1;
    return this.n;
  },
};

// The direct calls must keep answering — the widen moves the whole
// literal onto the any lane, receiver writes included.
console.log(o.read());
console.log(o.bump());
console.log(o.n);

// Detached: no receiver, so `this` is the global object.
const t = o.read;
console.log(t());

// An explicit thisArg rides argv[0] — this one used to be dropped
// silently before it crashed.
console.log(t.call(o));
console.log(t.call({ n: 9 }));

// The same read through a non-binding position.
const fns = [o.read];
console.log(fns[0].call(o));

function apply(f: any, r: any) {
  return f.call(r);
}
console.log(apply(o.read, o));
