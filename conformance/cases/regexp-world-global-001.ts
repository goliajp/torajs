// The RegExp constructor read off globalThis, by a spelled and by a
// computed key.
const R: any = (globalThis as any).RegExp;
console.log(new R("b").exec("abc")[0]);
const k = "Reg" + "Exp";
const G: any = (globalThis as any)[k];
console.log(typeof G, new G("c").test("abc"));
