// RFC 20260705 ledger #3 chunk 572 — remaining call-arg stations
// share: universal/namespace hasOwnProperty release owned temps only
// (an Ident key was consumed AND dropped = source stake destroyed,
// reuse-window UAF), Promise.resolve heap adopt shares a borrow-shaped
// arg, aggregate combinators borrow the promises array, Bun.file
// pass-through shares its path.
import { writeFile, unlink } from 'fs/promises'

// 1. struct hasOwnProperty(runtime key) — key survives the probe.
let obj = { alpha: 1 };
let k1 = "alp" + "ha";
console.log(obj.hasOwnProperty(k1));
let filler1 = "XXXX" + "YYYY";
console.log(k1, filler1);

// 2. namespace hasOwnProperty — runtime key survives the probe
// (runtime keys fold to false pre-lookup-table, bun-equal pick;
// literal key exercises the compile-time fold).
let k2 = "no" + "pe";
console.log(Number.hasOwnProperty(k2), k2);
console.log(Number.hasOwnProperty("NaN"));

// 3. Promise.resolve(heap value) shares — s survives after the
// promise drops in its block.
let s4 = "val" + 42;
{
  let p = Promise.resolve(s4);
}
let filler4 = "AAAA" + "BBBB";
console.log(s4, filler4);

// 4. Promise.all(arr) borrows the array — arr usable after.
let arr5 = [Promise.resolve(1), Promise.resolve(2)];
let all5 = Promise.all(arr5);
console.log(arr5.length);

// 5. Bun.file(path) shares the path — path survives the handle.
let p6 = "/tmp/tr-572-" + "probe.txt";
await writeFile(p6, "x");
{
  let t: string = await Bun.file(p6).text();
  console.log(t);
}
let filler6 = "CCCC" + "DDDD";
console.log(p6.length, filler6);
await unlink(p6);

// 6. fs args survive (path used after helper calls).
let p7 = "/tmp/tr-572-" + "second.txt";
await writeFile(p7, "y");
await unlink(p7);
console.log(p7);

// 7. owned-temp keys still release (no crash, correct values).
console.log(obj.hasOwnProperty("alp" + "ha"), obj.hasOwnProperty("be" + "ta"));
