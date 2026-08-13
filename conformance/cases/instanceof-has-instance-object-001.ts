const Odd: any = {};
Object.defineProperty(Odd, Symbol.hasInstance, { value: (x: any) => typeof x === "number" && x % 2 === 1 });
console.log("B1", (3 as any) instanceof Odd);
console.log("B2", (4 as any) instanceof Odd);

const Str: any = { [Symbol.hasInstance](x: any) { return typeof x === "string"; } };
console.log("E1", ("hi" as any) instanceof Str);
console.log("E2", (1 as any) instanceof Str);

const Truthy: any = { [Symbol.hasInstance](_x: any) { return "yes"; } };
console.log("I1", ({} as any) instanceof Truthy);

// receiver is a plain primitive (no `as any`) — the handler still decides
console.log("P1", 3 instanceof Odd);
console.log("P2", 4 instanceof Odd);

// handler reads `this`
const Self: any = { tag: 7, [Symbol.hasInstance](x: any) { return x === this.tag; } };
console.log("T1", (7 as any) instanceof Self);
console.log("T2", (8 as any) instanceof Self);

// RHS is not an object -> TypeError
const n: any = 42;
try { console.log("H1", ({} as any) instanceof n); } catch (e: any) { console.log("H1 threw", e.constructor.name); }

// RHS is an object with no handler and not callable -> TypeError
const plain: any = {};
try { console.log("H2", ({} as any) instanceof plain); } catch (e: any) { console.log("H2 threw", e.constructor.name); }

// handler that throws propagates
const Boom: any = { [Symbol.hasInstance](_x: any) { throw new RangeError("boom"); } };
try { console.log("H3", ({} as any) instanceof Boom); } catch (e: any) { console.log("H3 threw", e.constructor.name, e.message); }
