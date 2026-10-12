// a top-level function reading new.target and arguments.length, entered through Reflect.construct
function F(this: any, a: any, b: any) {
  console.log(new.target === F, new.target === G, arguments.length, a, b);
  this.n = arguments.length;
}
class G {}
const o: any = Reflect.construct(F, [1, 2]);
Reflect.construct(F, []);
const g: any = Reflect.construct(F, [1, 2, 3], G);
console.log(o.n, g.n, g instanceof G);
const K: any = F;
console.log(new K(5).n);
