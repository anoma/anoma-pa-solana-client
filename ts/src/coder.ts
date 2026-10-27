import { BorshCoder } from "@anchor-lang/core";

import { idl } from "./idl.js";

/** The adapter's accounts, events, instructions and types, encoded and decoded as its IDL describes them. */
export const coder = new BorshCoder(idl);
