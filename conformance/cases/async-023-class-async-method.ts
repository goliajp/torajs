// P10.3-A3a — class async method substrate acceptance fixture.
// Covers: instance async method, static async method, this.field
// access inside async body, Promise<T> return annotation, caller-
// side `await` + `.then` consumption.

class Greeter {
  prefix: string = "hello";

  async greet(name: string): Promise<string> {
    return this.prefix + " " + name;
  }

  static async fromGreeting(prefix: string): Promise<string> {
    return prefix + "!";
  }
}

async function run(): Promise<string> {
  const g = new Greeter();
  const r1: string = await g.greet("world");
  console.log(r1);

  const r2: string = await Greeter.fromGreeting("static-await");
  console.log(r2);

  return "done";
}

run().then((s: string) => { console.log(s); return s; });
