// RFC 20260805 D1b — an `await` in a loop head suspends: a for-of
// iterable (bound once), a `while` / `for` condition (re-awaited every
// iteration, `continue` included), a `switch` discriminant.
let ticks = 0;
const later = (v: any) => { ticks++; return Promise.resolve(v); };
async function main() {
  const seen: number[] = [];
  for (const x of await later([1, 2, 3])) { seen.push(x * 10); }
  console.log("for-of", seen.join(","), ticks);
  let n = 0;
  while (await later(n < 3)) { n++; if (n === 2) continue; seen.push(n); }
  console.log("while", n, seen.join(","), ticks);
  let m = 0;
  for (let i = 0; await later(i < 2); i++) { m += i + 1; }
  console.log("for", m, ticks);
  switch (await later("b")) {
    case "a": console.log("switch", "a"); break;
    case "b": console.log("switch", "b", ticks); break;
    default: console.log("switch", "default");
  }
  // the body of a head-awaiting loop can await too
  let total = 0;
  for (let i = 0; await later(i < 2); i++) { total += await later(i + 100); }
  console.log("nested", total, ticks);
}
main();
