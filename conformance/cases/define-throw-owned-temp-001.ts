// Define-family throw paths with an owned-temp receiver / descriptor /
// key / fresh dynobj alive at the throw point (rotation 549): every
// arm must answer the spec behavior, and the churn loops must keep
// answering it — the lowering parks those temps for the throw path
// (pre-549 they were stranded, 175-233MB over 600k iterations).

// primitive receivers / descriptors into each §20.1.2 / §28.1.2 entry
try { console.log('ok', Object.defineProperties("ab" as any, { p: { value: 1 } })); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Object.defineProperty("ab" as any, "p", { value: 1 })); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Object.defineProperty("ab" as any, "p", { get() { return 1; } })); } catch (e: any) { console.log('threw', e.constructor.name); }
const sd: any = "ab";
try { console.log('ok', Object.defineProperty({} as any, "p", sd)); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Reflect.defineProperty("ab" as any, "p", { value: 1 })); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Reflect.defineProperty({} as any, "p", sd)); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Object.defineProperties({} as any, "ab" as any)); } catch (e: any) { console.log('threw', e.constructor.name); }
try { console.log('ok', Object.create(null, "ab" as any)); } catch (e: any) { console.log('threw', e.constructor.name); }
const so: any = "ab";
const dd: any = { value: 1 };
try { console.log('ok', Object.defineProperty(so, "p", dd)); } catch (e: any) { console.log('threw', e.constructor.name); }

// owned-temp receiver alive across a throwing desc gate / key coerce /
// kernel refusal / props walk
const badKey: any = { toString() { throw new Error("k"); } };
const boom = (): any => { throw new Error("x"); };
let counts = [0, 0, 0, 0, 0, 0, 0];
for (let i = 0; i < 200; i++) {
  const d: any = "a" + (i % 10);
  try { Object.defineProperty({} as any, "p", d); } catch { counts[0]++; }
  try { Object.defineProperty(Object.freeze({}) as any, "p", { value: 1 }); } catch { counts[1]++; }
  try { Object.defineProperty({} as any, badKey, { value: 1 }); } catch { counts[2]++; }
  const p: any = { p: 5 };
  try { Object.defineProperties({} as any, p); } catch { counts[3]++; }
  try { Object.create({}, p); } catch { counts[4]++; }
  try { Reflect.defineProperty({} as any, "p", d); } catch { counts[5]++; }
  try { Reflect.defineProperty({} as any, "p", { value: 1 }, boom()); } catch { counts[6]++; }
}
console.log(counts.join(","));

// the normal paths still define
const o: any = {};
Object.defineProperty(o, "a", { value: 1, enumerable: true });
Reflect.defineProperty(o, "b", { value: 2, enumerable: true });
Object.defineProperties(o, { c: { value: 3, enumerable: true } });
const c = Object.create({ inherited: 1 }, { d: { value: 4, enumerable: true } });
console.log(JSON.stringify(o), JSON.stringify(c), c.inherited);
