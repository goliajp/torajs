// A class method's struct parameter is struct-shaped however the
// annotation spells it: through an alias of an alias, as an inline
// object type, or behind `| null`. Every spelling gets the all-any
// twin, so a rebound or any-held call with an argument that does not
// fit the layout reads `p.x` as the generic [[Get]] instead of through
// the struct layout.
type P = { x: number; y: number };
type Q = P;
type R = Q;

class C {
  alias(p: R): string { return "alias x=" + p.x; }
  inline(p: { x: number; y: number }): string { return "inline y=" + p.y; }
  nullable(p: P | null): string { return p ? "nullable x=" + p.x : "nullable nil"; }
  static sAlias(p: Q): string { return "sAlias y=" + p.y; }
  static sInline(p: { x: number; y: number }): string { return "sInline x=" + p.x; }
  static sNullable(p: P | null): string { return p ? "sNullable y=" + p.y : "sNullable nil"; }
}

const c = new C();
console.log(c.alias({ x: 1, y: 2 }), c.inline({ x: 3, y: 4 }), c.nullable({ x: 5, y: 6 }), c.nullable(null));
console.log(C.sAlias({ x: 1, y: 2 }), C.sInline({ x: 3, y: 4 }), C.sNullable({ x: 5, y: 6 }), C.sNullable(null));

const alias: any = c.alias.bind(c);
const inline: any = c.inline.bind(c);
const nullable: any = c.nullable.bind(c);
console.log(alias([7, 8]), alias("str"), alias(3));
console.log(inline([7, 8]), inline("str"), inline(3));
console.log(nullable([7, 8]), nullable("str"), nullable(3), nullable(null), nullable(undefined));

const sAlias: any = C.sAlias;
const sInline: any = C.sInline;
const sNullable: any = C.sNullable;
console.log(sAlias([7, 8]), sAlias("str"), sAlias(3));
console.log(sInline([7, 8]), sInline("str"), sInline(3));
console.log(sNullable([7, 8]), sNullable("str"), sNullable(3), sNullable(null), sNullable(undefined));

const held: any = c;
console.log(held.alias({ x: 9, y: 10 }), held.inline({ x: 11, y: 12 }), held.nullable({ x: 13, y: 14 }));
console.log(held.alias(new Proxy({ x: 15, y: 16 }, {})), held.nullable(new Proxy({ x: 17, y: 18 }, {})));
