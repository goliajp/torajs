// An async method of a class expression written inside a function
// suspends over its frame.

function viaConst() {
  const K = class {
    async m() {
      await 0;
      return 2;
    }
  };
  return new K().m();
}

function named() {
  const K = class Inner {
    async m(n: number) {
      let s = 0;
      for (let i = 0; i < n; i++) s += await Promise.resolve(i);
      return `${Inner.name} ${s}`;
    }
  };
  return new K().m(4);
}

function returned() {
  return class {
    static async make() {
      await null;
      return "made";
    }
  };
}

function capturing(tag: string) {
  const K = class {
    async m() {
      await 0;
      return tag + "!";
    }
  };
  return new K().m();
}

async function main() {
  console.log(await viaConst());
  console.log(await named());
  console.log(await returned().make());
  console.log(await capturing("cap"));
}

main();
