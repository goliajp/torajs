// An async body returning a class instance with a `then` method: the
// result promise adopts it rather than fulfilling with the instance.
class Later {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  then(ok: (v: number) => void): void {
    ok(this.n * 10);
  }
}

async function make(n: number): Promise<any> {
  return new Later(n);
}

make(4).then((v: any) => console.log("async adopted", v));
