import { describe, expect, it } from "vitest";
import {
  deriveKeyringSecrets,
  nullifierCommitment,
  signMessage,
} from "./walletKeyring";

const FIXTURE_IKM = new TextEncoder().encode("frontend-parity-fixture-ikm-32by");

function toHex(bytes: Uint8Array): string {
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

describe("walletKeyring", () => {
  it("matches the Rust mirror's parity fixture", () => {
    // These hex values are also asserted in
    // `rust/src/wallet_keyring.rs::tests::derive_keyring_secrets_matches_frontend_fixture`.
    // If you change KEYRING_SALT, the PRF domains, or the HKDF/HMAC chain,
    // both sides drift together — that's the point.
    const secrets = deriveKeyringSecrets(FIXTURE_IKM);
    expect(toHex(secrets.authority)).toBe(
      "942c7cd43cff3dc5419e70a99ff2ab11286ac0c2cba1bb9082079b6881640259"
    );
    expect(toHex(secrets.nullifier)).toBe(
      "dbd1ab0b7369154b26cf2b3bb03d1440239b90f274ae3647366fddaf72d34b10"
    );
    expect(toHex(secrets.encryption)).toBe(
      "d510bfa5078fe90e191687684c62f840c15472c134cadb29e2a747b72789d758"
    );
    expect(toHex(secrets.discovery)).toBe(
      "5fb211e18efda22b42124ca28f80727d9adb65d0240f93bd1859195fc91d0ed4"
    );
  });

  it("is deterministic for the same IKM", () => {
    const a = deriveKeyringSecrets(FIXTURE_IKM);
    const b = deriveKeyringSecrets(FIXTURE_IKM);
    expect(toHex(a.authority)).toBe(toHex(b.authority));
    expect(toHex(a.nullifier)).toBe(toHex(b.nullifier));
    expect(toHex(a.encryption)).toBe(toHex(b.encryption));
    expect(toHex(a.discovery)).toBe(toHex(b.discovery));
  });

  it("produces different keys for different IKM", () => {
    const a = deriveKeyringSecrets(FIXTURE_IKM);
    const b = deriveKeyringSecrets(
      new TextEncoder().encode("a different ikm of any byte length")
    );
    expect(toHex(a.authority)).not.toBe(toHex(b.authority));
    expect(toHex(a.nullifier)).not.toBe(toHex(b.nullifier));
  });

  it("nullifierCommitment is SHA-256 of the nk bytes", () => {
    const nk = new Uint8Array(32).fill(0xaa);
    expect(toHex(nullifierCommitment(nk))).toBe(
      "e0e77a507412b120f6ede61f62295b1a7b2ff19d3dcc8f7253e51663470c888e"
    );
  });

  it("signMessage format is stable", () => {
    expect(signMessage("FoobarWalletAddr111111111111111111111111111")).toBe(
      "I authorize AnomaPay to derive my account from address " +
        "FoobarWalletAddr111111111111111111111111111.\n" +
        "Do NOT sign this message if the request url is not " +
        "https://beta.anomapay.app/"
    );
  });
});
