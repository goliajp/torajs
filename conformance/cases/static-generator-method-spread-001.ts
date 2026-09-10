// ES §15.7.14 — inside a static member body `this` is the constructor
// object. The class-side forwarder the parser mints for a static
// generator method passed a `this` it never registered as a static
// site, so pass 2 spelled it `__this`, the forwarder grew a promoted
// `__this: any` head, and three things followed from that one head:
// `this === C` inside the generator answered false; the forwarder
// sat on no argv face (head-less declines a `__this` head, the
// method face declines `__sm_`), so every spread call site died at
// the callee boxing; and a value escape (`var ref = C.method`) killed
// the static face, after which `arguments.length` answered 0. The
// receiver is a registered static this site now (`C` after pass 2),
// and the forwarder rides the head-less argv face like a static
// non-generator method always did. Sync, async, and private static
// generators; spread sites with a trailing comma; the escaped value
// called with its own spread. (`this === C` is asserted on direct
// calls only: a bare fn-value call binds `this` to undefined, and
// tr's static `this` is the class-name mint on every call — the
// recorded static-this window, not this knife's.)
const arr = [2, 3];
let callCount = 0;
class C {
  static *method() {
    callCount = callCount + 1;
    return [arguments.length, arguments[0], arguments[3]];
  }
  static async *am() { return arguments.length; }
  static *#pm() { return [arguments.length, this === C]; }
  static run() { return C.#pm(42, ...[1], ...arr).next().value; }
  static *g() { return this === C; }
}
console.log(C.method(42, ...[1], ...arr,).next().value, C.g().next().value);
var ref = C.method;
console.log(ref(1, ...arr).next().value, callCount);
console.log(C.method(1, 2, 3).next().value);
console.log(C.method(...arr).next().value);
console.log(C.run());
C.am(42, ...[1], ...arr).next().then((r: any) => console.log(r.value));
