// as-cast out of the Any lane: string / boolean / bigint arms
// (pre-fix only `as number` unboxed; the rest passed NaN-box bits raw)
const b: any = "hi";
const s: string = b as string;
console.log(s);
console.log(s.length);

const n: any = 42;
const num: number = n as number;
console.log(num + 1);

const t: any = true;
const flag: boolean = t as boolean;
console.log(flag);
const f: any = false;
console.log(f as boolean);

const big: any = 123n;
const bi: bigint = big as bigint;
console.log(bi + 1n);

// direct argument position (no let binding catching the owned temp)
const raw: any = "world";
console.log(raw as string);

// substr view inside the box
const src: any = "hello world".slice(0, 5);
const sub: string = src as string;
console.log(sub);
