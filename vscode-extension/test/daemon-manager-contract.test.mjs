import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(testDirectory, "..", "src", "ipc", "DaemonManager.ts"),
  "utf8"
);

test("Linux daemon startup remains systemd-owned and authenticated", () => {
  assert.match(source, /probeAuthenticatedDaemon\(\)/);
  assert.match(source, /"event":"SessionStarted"/);
  assert.match(source, /cp\.execFile\(/);
  assert.match(
    source,
    /\["--user", "start", "--no-block", "aiosd\.service"\]/
  );

  const systemdStart = source.indexOf("await this.startViaSystemd()");
  const detachedSpawn = source.indexOf("cp.spawn(bin");
  assert.ok(systemdStart >= 0, "systemd startup path must exist");
  assert.ok(detachedSpawn > systemdStart, "detached spawn must remain a fallback");
});

test("a fresh token alone is never treated as daemon readiness", () => {
  assert.doesNotMatch(
    source,
    /if \(this\.isTokenFresh\(\)\) \{\s*return true;/
  );
  assert.match(source, /authenticated handshake succeeded/);
});
