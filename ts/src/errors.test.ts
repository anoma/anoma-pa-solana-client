import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { ANCHOR_ERROR_CODE_OFFSET, PA_ERRORS, paErrorCode, paErrorFromCode } from "./errors.js";

const idlErrors = (
  JSON.parse(
    readFileSync(fileURLToPath(new URL("../../idl/protocol_adapter.json", import.meta.url)), "utf8"),
  ) as { errors: { code: number; name: string }[] }
).errors;

describe("PaError", () => {
  it("has every error of the adapter's IDL under its name and code", () => {
    expect(PA_ERRORS.length).toBe(idlErrors.length);
    for (const { code, name } of idlErrors) {
      expect(paErrorFromCode(code), `${code}`).toBe(name);
      expect(paErrorCode(name as (typeof PA_ERRORS)[number]), name).toBe(code);
    }
  });

  it("decodes no code outside the adapter's errors", () => {
    expect(paErrorFromCode(ANCHOR_ERROR_CODE_OFFSET - 1)).toBeUndefined();
    expect(paErrorFromCode(ANCHOR_ERROR_CODE_OFFSET + PA_ERRORS.length)).toBeUndefined();
    expect(paErrorFromCode(ANCHOR_ERROR_CODE_OFFSET + 0.5)).toBeUndefined();
  });
});
