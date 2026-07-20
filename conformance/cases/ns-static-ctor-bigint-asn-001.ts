// RFC 20260720-ctor-static-reflection 刀 5b-2 — BigInt.asIntN /
// asUintN as VALUES (ns_static batch 3): reified-cell call with
// §7.1.22 ToIndex(bits) + §7.1.13 ToBigInt(value) coercions, name /
// length / print / gOPD reflection.

const f = BigInt.asIntN;
const g = BigInt.asUintN;

// ---- reified-cell calls (BigInt args) ----
console.log(f(8, 255n));                          // -1n
console.log(g(8, -1n));                           // 255n
console.log(f(64, 9223372036854775808n));         // -9223372036854775808n
console.log(g(128, -1n));                         // 340282366920938463463374607431768211455n

// ---- reflection ----
console.log(BigInt.asIntN.name, BigInt.asIntN.length);    // asIntN 2
console.log(BigInt.asUintN.name, BigInt.asUintN.length);  // asUintN 2
console.log(BigInt.asIntN);                       // [Function: asIntN]
const d = Object.getOwnPropertyDescriptor(BigInt, "asIntN");
console.log(d !== undefined && d.value === BigInt.asIntN); // true
console.log(d && (d as any).writable, d && (d as any).enumerable, d && (d as any).configurable); // true false true

// ---- ToIndex / ToBigInt coercions ----
console.log(f("8" as any, "255" as any));         // -1n   (string bits + string value)
console.log(f(8, true as any));                   // 1n
console.log(g(8, false as any));                  // 0n
console.log(f(8, { valueOf() { return 255n; } } as any)); // -1n  (ToPrimitive number hint)

// ---- rejects ----
// (direct try/catch — a closure-captured reified cell rides the
// separate typed-fn-slot boundary, not this fixture's surface)
try { f(8, 12 as any); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(8, 1.5 as any); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(8, null as any); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(8, undefined as any); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(8, "abc" as any); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(-1 as any, 1n); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { f(2 ** 53, 1n); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
