// sec 27.1.4.x IfAbruptCloseIterator -- a helper's argument
// validation failure closes the underlying iterator (return() is
// called, next is never read).
let closed = 0;
function closable() {
  return {
    __proto__: Iterator.prototype,
    get next() { throw new Error("next should not be read"); },
    return() { closed += 1; return {}; },
  };
}
try { (closable() as any).drop(-1); } catch (e) { console.log("caught", (e as any)?.constructor?.name); }
console.log("closed", closed);
try { (closable() as any).take(NaN); } catch (e) { console.log("caught", (e as any)?.constructor?.name); }
console.log("closed", closed);
try { (closable() as any).map(42); } catch (e) { console.log("caught", (e as any)?.constructor?.name); }
console.log("closed", closed);
// a ToNumber poison's own throw wins over the close
class Poison extends Error {}
try { (closable() as any).drop({ valueOf() { throw new Poison("p"); } }); } catch (e) { console.log("caught", (e as any)?.constructor?.name); }
console.log("closed", closed);
console.log("survived");
