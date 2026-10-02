import { defineConfig } from 'vitest/config';
import { availableParallelism } from 'node:os';

export default defineConfig({
  test: {
    include: ['tests/{unit,contract,integration}/**/*.test.ts'],
    testTimeout: 10000,
    hookTimeout: 15000,
    maxWorkers: Math.max(1, Math.min(2, availableParallelism() - 1, Number(process.env.NUMERA_TEST_WORKERS ?? process.env.HOOKMAKER_MAX_TEST_WORKERS ?? 2))),
    pool: 'forks',
    coverage: {
      provider: 'v8', include: ['src/**/*.ts'],
      reporter: ['text', 'json-summary', 'lcov'],
      thresholds: { statements: 85, branches: 80 },
    },
  },
});
