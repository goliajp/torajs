type Pair<A, B> = { fst: A, snd: B };
let p: Pair<number, number> = { fst: 5, snd: 10 };
let q: Pair<number, number> = { fst: 100, snd: 200 };
console.log(p.fst + p.snd);
console.log(q.fst + q.snd);
