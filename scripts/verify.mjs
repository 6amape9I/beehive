import { spawnSync } from "node:child_process";

function run(command, args, options = {}) {
  console.log(`\n> ${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    stdio: "inherit",
    shell: process.platform === "win32",
    ...options,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run("npm", ["run", "build"]);
console.log("\nRust tests on Windows require an MSVC Developer environment.");
console.log('For the canonical Stage 9 command, run:');
console.log('cmd.exe /c \'call "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Auxiliary\\Build\\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\\Cargo.toml\'');
run("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml"]);
