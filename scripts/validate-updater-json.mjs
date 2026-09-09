import { readFileSync } from "node:fs";

const [manifestPath, expectedVersion] = process.argv.slice(2);
if (!manifestPath || !expectedVersion) {
  throw new Error("usage: node scripts/validate-updater-json.mjs <latest.json> <version>");
}
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
if (manifest.version !== expectedVersion) {
  throw new Error(`latest.json version ${manifest.version ?? "missing"} does not match ${expectedVersion}`);
}
for (const platform of ["darwin-aarch64", "windows-x86_64"]) {
  const entry = manifest.platforms?.[platform];
  if (!entry?.url || !entry?.signature) {
    throw new Error(`latest.json is missing a signed updater entry for ${platform}`);
  }
}
console.log(`latest.json contains signed macOS and Windows updaters for ${expectedVersion}.`);
