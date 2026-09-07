import { defineConfig } from "@playwright/test";

// PNK_GATE_PORT lets several checkouts run the gate at once.
const PORT = Number(process.env.PNK_GATE_PORT ?? 8123);

export default defineConfig({
  testDir: "tests",
  timeout: 60_000,
  webServer: {
    command: `node_modules/.bin/esbuild --servedir=dist --serve=127.0.0.1:${PORT} --log-level=warning`,
    port: PORT,
    reuseExistingServer: true,
    timeout: 30_000,
  },
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // gate renders local fixtures only; videos/traces stay off
    trace: "off",
  },
});