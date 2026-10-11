// An array of a class that the any world also holds keeps its elements
// in any slots; a method call on one dispatches on the element itself,
// which may be an instance of another class with a method of that name.
class P {
  x = 1;
  m() {
    return "P";
  }
}
class Q {
  x = 2;
  m() {
    return "Q";
  }
}
function h(ps: P[]) {
  let s = "";
  for (const p of ps) s += p.m() + p.x;
  return s;
}
const g: any = h;
const mixed: any = [new P(), new Q()];
console.log(g(mixed));
const typed: P[] = [new P()];
const view: any = typed;
view.push(new Q());
console.log(h(typed), typed.length);

// the same through an indexed receiver
function k(ps: P[]) {
  let s = "";
  for (let i = 0; i < ps.length; i++) s += ps[i].m() + ps[i].x;
  return s;
}
const gk: any = k;
console.log(gk(mixed));
console.log(k(typed));
const typed2: P[] = [new P(), new P()];
const view2: any = typed2;
view2[1] = new Q();
console.log(k(typed2), typed2[1].m());
