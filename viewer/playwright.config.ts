import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests",
  timeout: 60_000,
  webServer: {
    command: "npm run serve",
    port: 8123,
    reuseExistingServer: true,
    timeout: 30_000,
  },
  use: {
    baseURL: "http://127.0.0.1:8123",
    // gate renders local fixtures only; videos/traces stay off
    trace: "off",
  },
});