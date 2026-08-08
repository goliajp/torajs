// RFC 20260809 刀 3 — `for (using x of …)` / `for (await using x of
// …)` per-iteration dispose (normal advance, break, continue), plus
// the `{ async [key]() {} }` computed method that used to stub-drop
// (an async [Symbol.asyncDispose] resource answered undefined).
const log: string[] = [];
function mk(t: string): any { return { [Symbol.dispose]() { log.push("d" + t); } }; }
const items: any = [mk("1"), mk("2"), mk("3")];
for (using r of items) {
  log.push("it");
}
console.log(log.join(","));

for (using r of [mk("b")] as any) {
  log.push("pre-break");
  break;
}
console.log(log.join(","));

let cnt = 0;
for (using r of [mk("c1"), mk("c2")] as any) {
  cnt = cnt + 1;
  continue;
}
console.log(cnt, log.join(","));

var am: any = { async [Symbol.asyncDispose]() { return 1; } };
console.log(typeof am[Symbol.asyncDispose]);

async function main(): Promise<void> {
  const xs: any = [
    { [Symbol.asyncDispose]() { log.push("ad1"); return Promise.resolve(0); } },
    { [Symbol.dispose]() { log.push("sd2"); } },
  ];
  for (await using r of xs) {
    log.push("ait");
  }
  console.log("await:", log.join(","));
}
main();
