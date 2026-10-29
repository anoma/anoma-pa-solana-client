import type { Idl, IdlType } from "@anchor-lang/core";

export function resolve(idl: Idl, type: IdlType): IdlType;

export function plain(idl: Idl, value: unknown, type: IdlType): unknown;
