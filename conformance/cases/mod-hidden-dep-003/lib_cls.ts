// Class-shaped hidden deps: a non-exported class an exported fn
// instantiates (`new` carries the name in a String field, not an
// Ident), and an exported class whose method leans on a non-exported
// helper fn. Classes can't mangle (parse-baked `__priv_` strings),
// so they inject bare when nothing collides.
class Hidden {
  v() { return 9; }
}
export function mkh() { return new Hidden().v(); }
function h() { return 5; }
export class K {
  w() { return h(); }
}
