// str/substr receiver: fresh view boxes owned; source survives
let s = "hello";
console.log(s?.[1], s?.[99], s);
// typed array element via optindex (borrow lane holds)
let arr: string[] = ["aa", "bb"];
let v1 = arr?.[0];
console.log(v1, arr?.[1], arr?.[5], arr[0]);
// any receiver: numeric + dynamic str key + literal key
let a: any = ["xx", "yy"];
console.log(a?.[0], a?.[3]);
let obj: any = { p: "pv", q: 7 };
let k = "p";
console.log(obj?.[k], obj?.["q"], obj?.[("m" as any) as string]);
// nullish receivers short-circuit (index never evaluates)
let n: any = null;
let u: string | null = null;
let hits = 0;
function idx(): number {
  hits++;
  return 0;
}
console.log(n?.[idx()], u?.[idx()], hits);
// let-position consume + chained use
let w = s?.[0];
console.log(w, s.length);
