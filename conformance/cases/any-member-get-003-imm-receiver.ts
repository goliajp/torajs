// int-imm receiver whose member name IS a class field (class-candidate lane)
class Pt { x: number = 1; }
const p = new Pt();
console.log(p.x);
console.log((42 as any).x);
const pa: any = p;
console.log(pa.x);
const s: any = "hello";
console.log(s.nothere);
console.log(typeof s.toUpperCase);
const e: any = new TypeError("boom");
console.log(e.message);
