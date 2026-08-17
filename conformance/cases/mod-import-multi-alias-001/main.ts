// 421-04 — one imported name, several importer-visible spellings:
// `import { fa, fa as renamed }` binds BOTH (a single-alias rename
// map collapsed them to the last spelling). The plain spelling keeps
// the original decl so recursive self-references stay bound; extra
// fn spellings are deep clones, extra const spellings are reference
// bindings. The re-export and star-forward lanes fan out the same
// way.
import { fa, fa as renamed, fb as g1, fb as g2 } from "./lib";
import { f1, f2 } from "./mid.ts";
import { fact as sf1, fact as sf2 } from "./star.ts";
console.log(fa, renamed, g1(), g2());
console.log(f1(4), f2(3));
console.log(sf1(3), sf2(5));
