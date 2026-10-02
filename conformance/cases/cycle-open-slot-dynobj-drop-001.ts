// a literal promoted to a dynobj by defineProperty comes back through
// a struct-typed return into an eval-hoisted var; while unshift throws
// the runtime buffers it as a cycle root, and the typed drop of that
// open slot must hand the cell to its own tag's drop (store, entries,
// buffer entry) instead of freeing the header alone: the dangling
// buffer entry crashed the exit drain once the allocator reused the
// block
function check(actual: boolean, expected: boolean): void { if (actual !== expected) throw new Error("x"); }
function obj() {
  var o = {0: 1, 1: 2, 2: 3, length: 3};
  Object.defineProperty(o, 0, {writable: false});
  return o;
}
function ogap() {
  var o = {0: 1, 1: 2, /* no 2 */ 3: 4, length: 4};
  Object.defineProperty(o, 3, {configurable: false});
  return o;
}
check(((() => { try { eval('\'use strict\'; var o = obj(); [Array.prototype.unshift.call(o, 40, 50), o]'); return false; } catch (e) { return e instanceof (TypeError); } })() && (() => { try { eval('var o = obj(); [Array.prototype.unshift.call(o, 40, 50), o]'); return false; } catch (e) { return e instanceof (TypeError); } })()),
         true);
check(((() => { try { eval('\'use strict\'; var o = ogap(); [Array.prototype.unshift.call(o, 9), o]'); return false; } catch (e) { return e instanceof (TypeError); } })() && (() => { try { eval('var o = ogap(); [Array.prototype.unshift.call(o, 9), o]'); return false; } catch (e) { return e instanceof (TypeError); } })()),
         true);

console.log("end");
