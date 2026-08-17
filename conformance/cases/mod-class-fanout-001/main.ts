// 423-01 knife D1 — `import { Kf, Kf as K2 }` binds BOTH spellings
// to the ONE class identity: the extra spelling is a reference
// binding (`const K2 = Kf`), so instanceof crosses spellings and the
// class values compare equal (a deep clone would split the brand).
import { Kf, Kf as K2 } from "./lib_kf.ts";
console.log(new Kf().v(), new K2().v());
console.log(new Kf() instanceof K2, new K2() instanceof Kf, Kf === K2);
