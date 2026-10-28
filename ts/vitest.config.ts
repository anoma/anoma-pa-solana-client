import { defineConfig } from "vitest/config";

// `unit` runs on every push (`npm test`); `deployed` reads the recorded
// deployments over a Helius endpoint (`npm run test:deployed`).
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: "unit",
          include: ["src/**/*.test.ts"],
          exclude: ["src/**/*.deployed.test.ts"],
        },
      },
      {
        test: {
          name: "deployed",
          include: ["src/**/*.deployed.test.ts"],
          testTimeout: 60_000,
        },
      },
    ],
  },
});
