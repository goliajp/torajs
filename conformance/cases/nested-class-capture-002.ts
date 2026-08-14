// Each call of the enclosing function mints its own class, closed
// over that call's environment — the identity question a static
// class model cannot answer.
function outer(a: number): number {
  class K {
    m(): number {
      return a;
    }
  }
  return new K().m();
}
console.log(outer(7), outer(9));

function mk(a: number): any {
  class K {
    m(): number {
      return a * 10;
    }
  }
  return new K();
}
const x = mk(1);
const y = mk(2);
console.log(x.m(), y.m());
