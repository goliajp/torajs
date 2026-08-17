// rotation 430 — a module requested back by its own import chain
// re-parses into the arena; the discarded copy's fn literals still
// ride the whole-arena lift as construction-less orphans (their
// captures name census-mangled bindings that were never declared).
// Pass 2B must shelve them as dead stubs instead of panicking on
// the missing capture side-channel.
import { B } from "./f.ts";
export let A = 3;
let local = 9;
const g = () => {
  console.log(typeof local, B);
};
g();
