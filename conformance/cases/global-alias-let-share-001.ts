// Binding a promoted top-level global into a local `let` / `const`
// takes a SHARE of the global's cell, not ownership of it.
//
// The global read is a bare slot load (`GlobalRef + Load`), so it
// borrows. The let-decl's ownership table used to consult only
// `ctx.locals`, and a global is in neither map: the binding got
// neither the share-inc nor the alias mark, so it owned a stake it
// never took and its scope-end drop freed the program's only copy.
// Every later read of the global then ran on reused memory —
// silently wrong first (`.length` off a recycled cell), then fatal
// once a push grew a Vec off the garbage capacity.
//
// The assignment side already had this row
// (`ssa_lower_assign_ident`, chunk 558); this is the declaration
// side of the same ledger. Each probe below reads its global AFTER
// heap churn has had the chance to reuse a freed cell.

const ARR: any[] = [1, 2, 3, 4, 5, 6, 7, 8];
const ARR2: any[] = [1, 2, 3, 4, 5, 6, 7, 8];
const ARR3: any[] = [1, 2, 3, 4, 5, 6, 7, 8];
const ARR4: any[] = [1, 2, 3, 4, 5, 6, 7, 8];
const STR: string = "abcdefgh";
const OBJ: any = { a: 1, b: 2 };

// plain `let` alias, never reassigned
function aliasLet(): number {
  let cur: any[] = ARR;
  return cur.length;
}

// `const` alias
function aliasConst(): number {
  const cur: any[] = ARR2;
  return cur.length;
}

// un-annotated alias
function aliasNoAnn(): number {
  let cur = ARR3;
  return cur.length;
}

// alias then reassigned to a fresh array — the source global keeps
// its own stake through the drop-old of the reassignment
function aliasReassigned(): number {
  let cur: any[] = ARR4;
  const picked: any[] = [];
  picked.push(9);
  cur = picked;
  return cur.length;
}

function aliasStr(): number {
  const s: string = STR;
  return s.length;
}

function aliasObj(): number {
  const o: any = OBJ;
  return o.a + o.b;
}

// the `as`-cast arm reads the same global through a value-layer
// pass-through and owes the same share
function aliasAsCast(): number {
  const cur: any = ARR as any;
  return cur.length;
}

console.log(aliasLet(), aliasConst(), aliasNoAnn(), aliasReassigned());
console.log(aliasStr(), aliasObj(), aliasAsCast());

function churn(): void {
  for (let i = 0; i < 400; i++) {
    const junk: any[] = [1, 2, 3, 4, 5, 6, 7, 8];
    junk.push(i);
    junk.push(i);
    const js: string = "junk" + i;
    if (js.length === 0) {
      console.log("unreachable");
    }
  }
}

churn();

console.log(ARR.length, ARR2.length, ARR3.length, ARR4.length);
console.log(ARR[0], ARR[7], ARR2[7], ARR3[7], ARR4[7]);
console.log(STR.length, STR, OBJ.a, OBJ.b);

// the shape that first surfaced this: a driver called twice, each
// call aliasing the same global list into a local before narrowing
// it, with allocation in between
const FACTORIES: any[] = [1, 2, 3, 4, 5, 6, 7, 8];

function driver(keepFirst: boolean): number {
  let chosen: any[] = FACTORIES;
  if (keepFirst) {
    const picked: any[] = [];
    picked.push(FACTORIES[0]);
    chosen = picked;
  }
  const sink: any[] = [];
  for (let i = 0; i < 50; i++) {
    sink.push(i);
  }
  return chosen.length + sink.length;
}

console.log(driver(true), FACTORIES.length);
console.log(driver(true), FACTORIES.length);
console.log(driver(false), FACTORIES.length);
