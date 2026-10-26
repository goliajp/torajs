// An element of a function array read behind a non-null assertion is
// called directly (`hs.at(i)!(x)`), with and without an `any` function
// stored in the array; an `undefined` element so called throws.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const mk = (): F[] => [(x: number) => x + 1, (x: number) => x * 2, (x: number) => x - 3];

const plain = mk();
console.log(plain.at(0)!(1), plain.at(-1)!(2), plain[1]!(3));
console.log(plain.find((f) => f(2) === 4)!(5), plain.findLast((f) => true)!(6));
console.log(plain.pop()!(7), plain.shift()!(8), plain.length);

const hs = mk();
hs[0] = tag;
console.log(hs.at(0)!(1), hs.at(1)!(2), hs[0]!(3));
console.log(hs.shift()!(4), hs.pop()!(5), hs.length);

const ns: (F | undefined)[] = [undefined, (x: number) => x * 10];
console.log(ns.at(1)!(4));
try {
  ns.at(0)!(1);
} catch (e) {
  console.log(e instanceof TypeError);
}

const s: number = ns.at(1)!(1) + 1;
console.log(s * 2, ns.at(1)!(2) * 3, Math.floor(ns[1]!(0.25)));
ns.at(1)!(1);
const ms: (F | undefined)[] = [undefined, (x: number) => x * 10];
ms[0] = tag;
console.log(ms.at(0)!(1), ms.at(1)!(1), ms.pop()!(3));
