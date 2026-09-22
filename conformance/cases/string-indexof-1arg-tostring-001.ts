// §22.1.3.9 / §22.1.3.11 step 3 — the 1-arg form of indexOf / lastIndexOf
// runs ToString on the search value, whatever its type.
const s = "a1true0NaN,[object Object]undefinednull-0";
console.log(s.indexOf(true), s.lastIndexOf(true));
console.log(s.indexOf(0), s.indexOf(-0), s.indexOf(NaN));
console.log(s.indexOf([]), s.indexOf({}), s.indexOf(0n));
console.log(s.indexOf(null), s.indexOf(undefined), s.lastIndexOf(null));
console.log(s.indexOf({ toString() { return "true"; } }));
console.log(s.indexOf([1]), s.lastIndexOf(["a", 1]));
try { s.indexOf(Symbol("x")); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
try { s.lastIndexOf({ toString() { throw new RangeError("r"); } }); } catch (e) { console.log(e instanceof RangeError); }
console.log(s.indexOf(new String("true") as any));
console.log("abc".indexOf("c"), "abcabc".lastIndexOf("b"));
