// Compile one OverPy file with the pinned upstream compiler.
// usage: node compile.js <source.opy> <out.txt>; prints one JSON line {ok, error?}.
const overpy = require("overpy");
const fs = require("fs");
const path = require("path");

(async () => {
  await overpy.readyPromise;
  const [source, out] = process.argv.slice(2);
  try {
    const result = await overpy.compile(fs.readFileSync(source, "utf8"), "en-US", path.dirname(path.resolve(source)), path.basename(source));
    fs.writeFileSync(out, result.result);
    console.log(JSON.stringify({ ok: true }));
  } catch (e) {
    console.log(JSON.stringify({ ok: false, error: String((e && e.message) || e).split("\n")[0] }));
    process.exit(1);
  }
})();
