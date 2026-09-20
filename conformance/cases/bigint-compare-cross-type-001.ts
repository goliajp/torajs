// §7.2.13 IsLessThan is defined ACROSS the numeric types: after
// ToPrimitive it takes ToNumeric of both sides and step 5 compares a
// BigInt against a Number by mathematical value. So comparison does
// not throw for a mixed pair the way arithmetic does — it answers.
//
// The one exception is a Symbol: ToPrimitive hands it back and
// ToNumeric refuses it. That refusal is decidable while compiling,
// which makes it band B.

let bi: bigint = 2n;
let nu: number = 1;
let bo: boolean = true;
let st: string = "3";

console.log("vs number:", bi > nu, bi < nu, bi >= 2, bi <= 1);
console.log("vs boolean:", bi > bo, bi < bo);
console.log("vs string:", bi > st, bi < st);
console.log("vs null:", bi > (null as any), bi < (null as any));

// Equality never throws either (§7.2.15 answers across the types).
console.log("equality:", (1n as any) == 1, (2n as any) === 2);

// A Symbol on either side is the one shape that throws.
const sym: any = Symbol("s");
try {
    console.log(bi < sym);
} catch (e: any) {
    console.log("sym right:", e.constructor.name);
}
try {
    console.log(sym > bi);
} catch (e: any) {
    console.log("sym left:", e.constructor.name);
}
