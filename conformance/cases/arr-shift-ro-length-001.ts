// §23.1.3.29 shift on a receiver whose length is not writable: every
// element moves down and the last index is deleted before the final
// length write refuses. A seal fails the delete instead, after the moves.

const a: number[] = [5, 6];
Object.defineProperty(a, "length", { writable: false });
try { a.shift(); } catch (e) { console.log((e as Error).constructor.name, a.join(","), a.length, 1 in a); }
const b: number[] = [7];
Object.defineProperty(b, "length", { writable: false });
try { b.shift(); } catch (e) { console.log((e as Error).constructor.name, b.join(","), b.length, 0 in b); }
const c: string[] = ["x", "y", "z"];
Object.defineProperty(c, "length", { writable: false });
try { c.shift(); } catch (e) { console.log((e as Error).constructor.name, c.join(","), c.length, 2 in c); }
const d: any[] = [{ k: 1 }, { k: 2 }];
Object.defineProperty(d, "length", { writable: false });
try { d.shift(); } catch (e) { console.log((e as Error).constructor.name, d.map(x => x === undefined ? "u" : x.k).join(","), 1 in d); }
const s: number[] = [1, 2, 3];
Object.seal(s);
Object.defineProperty(s, "length", { writable: false });
try { s.shift(); } catch (e) { console.log((e as Error).constructor.name, s.join(","), s.length); }
