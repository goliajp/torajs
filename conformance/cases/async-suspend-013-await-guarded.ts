// RFC 20260805 D1b — an `await` on the guarded side of `&&` / `||` /
// `??` or in a ternary branch takes the statement form: the guard is
// evaluated first, the awaited side only when the guard says so, and
// the function still suspends there.
let hits = 0;
const p = (v: any) => { hits++; return Promise.resolve(v); };
async function main() {
  const a = (true) ? await p("t") : "no";
  const b = (false) ? await p("x") : await p("e");
  console.log("ternary", a, b, hits);
  const c = 0 && (await p("never"));
  const d = 1 && (await p("and"));
  console.log("and", c, d, hits);
  const e = "left" || (await p("never"));
  const f = null || (await p("or"));
  console.log("or", e, f, hits);
  const g = undefined ?? (await p("nullish"));
  const h = 0 ?? (await p("never"));
  console.log("nullish", g, h, hits);
  // nested: a guarded await inside a call argument, with a prior operand read before the suspension
  let order = "";
  const q = (async () => { await p(1); order += "a"; return "q"; })();
  order += "s";
  console.log("nested", order, true ? await q : "-", order);
  if ((await p(false)) || (await p(true))) { console.log("cond", "reached", hits); }
}
main();
