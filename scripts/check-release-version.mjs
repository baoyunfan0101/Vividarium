import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const tag = process.argv[2];
const read = (path) => readFileSync(resolve(root, path), "utf8");
const workspaceVersion = read("Cargo.toml").match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const packageVersion = JSON.parse(read("apps/desktop/package.json")).version;
const lockVersion = JSON.parse(read("apps/desktop/package-lock.json")).packages[""].version;
const metadata = JSON.parse(execFileSync(
  "cargo",
  ["metadata", "--locked", "--no-deps", "--format-version", "1"],
  { cwd: root, encoding: "utf8" },
));
const localVersions = metadata.packages
  .filter((pkg) => ["vividarium-core", "vividarium-desktop"].includes(pkg.name))
  .map((pkg) => pkg.version);

if (!workspaceVersion || !packageVersion || !lockVersion) {
  throw new Error("release version metadata is incomplete");
}
if (new Set([workspaceVersion, packageVersion, lockVersion, ...localVersions]).size !== 1) {
  throw new Error(`release version mismatch: ${[
    `workspace=${workspaceVersion}`,
    `package=${packageVersion}`,
    `package-lock=${lockVersion}`,
    ...metadata.packages
      .filter((pkg) => ["vividarium-core", "vividarium-desktop"].includes(pkg.name))
      .map((pkg) => `${pkg.name}=${pkg.version}`),
  ].join(", ")}`);
}
if (tag && tag !== `v${packageVersion}`) {
  throw new Error(`release tag ${tag} does not match v${packageVersion}`);
}
console.log(`Release version ${packageVersion} is consistent.`);
