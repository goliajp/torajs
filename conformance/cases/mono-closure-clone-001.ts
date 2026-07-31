// RFC 20260731-mono-closure-clone — lifted closures inside a generic
// (implicit) fn body must clone per specialization: the shared-decl
// form let one spec's env_drop walk another spec's env layout
// (boolean capture slot dropped as a heap pointer → SIGSEGV at 0x7).

// objlit computed-key method capturing the untyped param, heap +
// non-heap call sites, reassignment drops the first spec's cell
function MakeIterable(iterator) {
  return {
    [Symbol.iterator]() {
      return iterator;
    }
  };
}
var iterator;
iterator = Iterator.concat(MakeIterable(true));
try { iterator.next(); } catch (e) { console.log("A", e instanceof TypeError); }
iterator = Iterator.concat(MakeIterable(123n));
try { iterator.next(); } catch (e) { console.log("B", e instanceof TypeError); }
iterator = Iterator.concat(MakeIterable("abcdefghijklmnop"));
try { iterator.next(); } catch (e) { console.log("C", e instanceof TypeError); }

// method-shorthand variant, drop via reassignment both directions
function mk(x) {
  return { m() { return x; } };
}
var h = mk(true);
console.log("D", h.m());
var h2 = mk(42n);
console.log("E", typeof h2.m());
h = mk(false);
console.log("F", h.m());
console.log("G", typeof h2.m());

// arrow variant with two specs and a call after the first drops
function wrap(v) {
  return () => v;
}
var f1 = wrap("heap-string-payload");
var f2 = wrap(7);
console.log("H", f1(), f2());
f1 = wrap("second");
console.log("I", f1(), f2());
