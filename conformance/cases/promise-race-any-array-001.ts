// Promise.race over Array<Any> (mixed promise / plain-value slots) —
// §27.2.4.5. First settled element in array order wins; a non-promise
// element is an already-fulfilled value and wins on sight.
const a: any[] = [Promise.resolve(1), 2, Promise.resolve(3)];
Promise.race(a).then((v: any) => console.log("race first-fulfilled:", v));

const b: any[] = [Promise.reject("bad"), Promise.resolve("good")];
Promise.race(b).catch((e: any) => console.log("race first-rejected:", e));

const c: any[] = ["plainstr", Promise.resolve(9)];
Promise.race(c).then((v: any) => console.log("race plain-value wins:", v));
