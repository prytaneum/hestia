import { deepStrictEqual } from "node:assert";

const wit = Bun.spawnSync(["bun", "x", "--bun", "jco", "wit", "dist/plugin.wasm"]).stdout.toString();
const lines = wit.split("\n").map((line) => line.trim());

deepStrictEqual(
    lines.filter((line) => line.startsWith("import ") && /wasi:(http|sockets)|hestia:kernel\/(download|programs|ipc)/.test(line)),
    [],
);
deepStrictEqual(
    lines.filter((line) => line.startsWith("export ")),
    ["export hestia:kernel/lifecycle@0.1.0;"],
);
