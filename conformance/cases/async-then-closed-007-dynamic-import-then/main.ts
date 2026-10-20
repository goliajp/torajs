// A dynamic import settles with the namespace, a thenable when its
// module exports `then` (ECMA-262 §13.3.10.1, §27.2.1.3.2).

async function main() {
  const v = await import("./lib");
  console.log(typeof v, String(v));
}

main();
