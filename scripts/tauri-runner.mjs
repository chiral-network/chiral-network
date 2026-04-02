import { spawn } from "node:child_process";

const [, , command] = process.argv;

if (!command || (command !== "dev" && command !== "build")) {
  console.error("Usage: node scripts/tauri-runner.mjs <dev|build>");
  process.exit(1);
}

const env = { ...process.env };

// Work around Windows App Control policies that block binaries in workspace paths.
if (process.platform === "win32" && !env.CARGO_TARGET_DIR && env.LOCALAPPDATA) {
  env.CARGO_TARGET_DIR = `${env.LOCALAPPDATA}\\chiral-cargo-target`;
}

const child =
  process.platform === "win32"
    ? spawn("cmd.exe", ["/d", "/s", "/c", "tauri", command], {
        stdio: "inherit",
        env,
      })
    : spawn("tauri", [command], {
        stdio: "inherit",
        env,
      });

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
