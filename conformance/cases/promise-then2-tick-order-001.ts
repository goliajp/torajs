Promise.resolve(1).then((v: any) => console.log("A", v));
Promise.reject(2).then((v: any) => console.log("never"), (e: any) => console.log("B", e));
Promise.resolve(3).then((v: any) => console.log("C", v));
const one: any[] = [1];
Promise.all(one).then((v: any) => console.log("ALL"));
Promise.resolve(0).then((v: any) => console.log("P"));
