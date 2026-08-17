// A lib whose exports lean on top-level names nobody imports: a
// plain helper, a recursive chain, and an exported-but-unrequested
// util. All of them must ride along (mangled, never importer-
// visible) when only `c` / `d` / `e` are asked for.
function mk() { return 7; }
export const c = mk();
function h2() { return 3; }
function mk2() { return h2() + 4; }
export const d = mk2();
export function util() { return 11; }
export const e = util();
