// A module namespace whose module exports a function under the name
// `then` (ECMA-262 §10.4.6, §27.2.1.3.2 steps 9-13): awaiting the
// namespace calls it.
import * as ns from "./lib";

async function main() {
  const v = await ns;
  console.log(typeof v, String(v));
}

main();
