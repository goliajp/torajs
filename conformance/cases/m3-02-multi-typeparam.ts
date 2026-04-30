function fst<A, B>(a: A, b: B): A { return a; }
function snd<A, B>(a: A, b: B): B { return b; }
console.log(fst(7, "ignored"));
console.log(snd("first", 42));
