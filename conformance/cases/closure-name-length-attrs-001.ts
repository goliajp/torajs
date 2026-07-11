// RFC 20260711-closure-reflection chunk C — fn `name` / `length`
// non-writable + configurable protocol (ES §20.2.4 attributes).
//
// - strict assign to either throws TypeError, the value holds
//   (unconditional — after a delete the set walks to
//   Function.prototype's own non-writable pair and refuses the
//   same way);
// - delete succeeds (configurable: true) and tombstones the
//   virtual own property: hasOwnProperty flips false, gOPD answers
//   undefined, the sibling prop is untouched;
// - interned proto method cells share the tombstone process-wide
//   (the spec object is a singleton — same in bun);
// - ordinary expando keys stay writable.
//
// Acceptance: byte-equal with bun. (The post-delete `f.name` READ
// value stays a recorded divergence — bun walks the proto chain to
// Function.prototype.name; tr answers undefined — so this fixture
// asserts through hasOwnProperty / gOPD instead.)

function show(d: any) {
  if (d === undefined) { console.log("undefined"); return; }
  console.log(d.value, d.writable, d.enumerable, d.configurable);
}

function named(a: number, b: number) { return a + b; }
const f: any = named;

// 1. non-writable — assign throws, value holds
try { f.name = "hacked"; } catch (e) { console.log("name-write-threw", e instanceof TypeError); }
console.log("name-after", f.name);
try { f.length = 99; } catch (e) { console.log("len-write-threw", e instanceof TypeError); }
console.log("len-after", f.length);

// 2. configurable — delete removes the own prop
console.log("del-len", delete f.length, "has-after", f.hasOwnProperty("length"));
show(Object.getOwnPropertyDescriptor(f, "length"));

// 3. post-delete assign still refuses (Function.prototype's pair)
try { f.length = 5; } catch (e) { console.log("recreate-threw", e instanceof TypeError); }
console.log("no-own-after-recreate", f.hasOwnProperty("length"));

// 4. sibling prop untouched; then delete it too
console.log("name-still", f.name, f.hasOwnProperty("name"));
console.log("del-name", delete f.name, f.hasOwnProperty("name"));
show(Object.getOwnPropertyDescriptor(f, "name"));

// 5. interned method cell tombstone (spec singleton)
const sp: any = String.prototype.slice;
console.log("sp-del", delete sp.name, sp.hasOwnProperty("name"));

// 6. ordinary expando keys stay writable
f.custom = 1;
console.log("custom", f.custom);
