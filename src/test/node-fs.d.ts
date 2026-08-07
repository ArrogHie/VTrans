/**
 * Minimal ambient types for `node:fs` used by style-assertion tests.
 *
 * The workspace has no `@types/node` dependency and `tsconfig.json` pins
 * `types` to `vitest/globals`, so the two functions this test suite needs
 * are declared here instead of widening the project type surface.
 */
declare module "node:fs" {
  export function readFileSync(path: string | URL, encoding?: string): string;
}
