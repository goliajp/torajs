// A list's lexical bindings exist from the list's entry (§14.2.2,
// §14.12.4), so a closure made before a declaration has run names the
// list's binding, in a switch CaseBlock, a try body or a method body
// as in a block. A nested block's binding of the same name is that
// block's own and ends with it. A `var` is the function's binding from
// the function's entry (§10.2.11), undefined until its declaration runs.
const k = "o";

function sw(n: number): number {
  switch (n) {
    case 0: {
    }
    case 1:
      const g = () => k;
      const k = 2;
      return g();
  }
  return 0;
}

function tr(): number {
  try {
    const g = () => k;
    const k = 3;
    return g();
  } finally {
  }
}

function blockTdz(): string {
  try {
    {
      const g = () => k;
      g();
      const k = 1;
    }
    return "no";
  } catch (e: any) {
    return e.constructor.name;
  }
}

class C {
  m(): number {
    const g = () => k;
    const k = 4;
    return g();
  }
}

function leak(): number {
  {
    {
      let k = "s";
    }
    const g = () => k;
    const k = 5;
    return g();
  }
}

function viaVar(): any {
  const g = () => k;
  var k = 6;
  return g();
}

console.log(sw(1), tr(), blockTdz(), new C().m(), leak(), viaVar());

// a nested block's binding of the name neither fills nor retypes the
// binding a closure of the enclosing list captured ahead of it
function inner(): string {
  const out: (number | string)[] = [];
  {
    const g = () => k;
    {
      let k = 9;
      out.push(k);
    }
    const k = 5;
    out.push(g());
  }
  {
    const g = () => k;
    {
      let k = "s";
      out.push(k);
    }
    const k = 6;
    out.push(g() * 2);
  }
  return out.join(",");
}

function inClause(n: number): string {
  const g = () => k;
  switch (n) {
    case 0: {
      const k = "c";
      return k;
    }
  }
  const k = "body";
  return g();
}

// the closure's return type is the binding's own, not that of an
// unrelated binding of the name elsewhere in the program
function unrelated(): string {
  {
    {
      const g = () => k;
      const h = (): string => {
        const k = "s";
        const q = () => k;
        return q();
      };
      const k = 7;
      return g() + h();
    }
  }
}

console.log(inner(), inClause(0), inClause(1), unrelated());

// a `var` the closure reads before its declaration ran
function varEarly(): any {
  const g = () => k;
  const before = g();
  var k = 1;
  return [before, g()];
}

function varNested(): any {
  const g = () => k;
  {
    var k = 2;
  }
  return g();
}

function varHead(): number[] {
  const out: number[] = [];
  for (var g = () => k, k = 0; k < 2; k++) out.push(g());
  return out;
}

const h0 = (): number => {
  const g = () => k * 2;
  var k = 7;
  return g();
};

console.log(varEarly(), varNested(), varHead(), h0());
