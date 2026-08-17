// Hidden deps that carry state and faces: a mutable counter shared
// by two exported fns, a bare-exported const another export reads,
// and a default expression built from a helper.
var n = 0;
export function inc() { n += 1; return n; }
const a = 5;
export { a as b };
export const c = a * 2;
function mkDefault() { return 9; }
export default mkDefault();
