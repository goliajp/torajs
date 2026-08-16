// Leaf of the star-re-export chain (mod-star-reexport-001).
export const A_VAL = "a-val";
export function fa(): string { return "fa-result"; }
export class Ca { m(): string { return "Ca.m"; } }
// Exported by BOTH this module and the hub above it — §16.2.3 resolves
// a name against the importing module's own export entries before it
// looks through a star, so the hub's spelling has to win.
export const shared = "from-a";
