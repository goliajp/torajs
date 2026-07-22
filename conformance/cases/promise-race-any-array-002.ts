// Promise.any over Array<Any> (mixed promise / plain-value slots) —
// §27.2.4.2. First FULFILLED element wins; a rejected element is
// skipped in favour of a later fulfilled one.
const a: any[] = [Promise.resolve(1), 2, Promise.resolve(3)];
Promise.any(a).then((v: any) => console.log("any first-fulfilled:", v));

const b: any[] = [Promise.reject("bad"), Promise.resolve("good")];
Promise.any(b).then((v: any) => console.log("any skips rejection:", v));

const c: any[] = [42, Promise.reject("late")];
Promise.any(c).then((v: any) => console.log("any plain-value wins:", v));
