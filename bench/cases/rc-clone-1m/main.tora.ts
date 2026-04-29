type Pair = { a: number, b: number };

function consume(x: Rc<Pair>): number {
  return x.a;
}

let u: Rc<Pair> = Rc.new({ a: 1, b: 2 });
let i: number = 0;
let acc: number = 0;
while (i < 10000000) {
  acc = acc + consume(u.clone());
  i = i + 1;
}
console.log(acc);
