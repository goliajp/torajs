// throwing a cross-scope alias binding: the alias escapes via throw
// (retain-at-throw hands the catch its own +1) while the owner stays
// readable after the catch.
function main(): void {
  const msg: string = "thrown-alias";
  try {
    {
      const m: string = msg;
      throw m;
    }
  } catch (e) {
    console.log(e);
  }
  console.log(msg);
}

main();
