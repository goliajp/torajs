// An `any[]` element handed to a callback parameter annotated with a
// scalar type binds as it is (§10.2.1.3): the callback sees the real
// value, not one converted to the annotation.
const a: any[] = [1, "s", { k: 1 }, true, "5"];
console.log(a.map((v: number) => typeof v).join(","));
a.forEach((v: number) => console.log("forEach", typeof v));
console.log(a.filter((v: number) => typeof v === "string").length);
console.log(a.reduce((acc: string, v: number) => acc + typeof v + ";", ""));
console.log(a.map((v: string) => typeof v).join(","));
console.log(a.map((v: boolean) => typeof v).join(","));
const nums: any[] = [1, 2.5, 3];
console.log(nums.map((v: number) => v * 2).join(","));
