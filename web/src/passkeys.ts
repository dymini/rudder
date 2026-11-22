import { api } from "./api";
import type { SessionResponse } from "./types";

type Challenge = { challenge_id: string; options: { publicKey: unknown } };
type CredentialJSON = {
  id: string;
  rawId: string;
  type: string;
  response: Record<string, unknown>;
  extensions: AuthenticationExtensionsClientOutputs;
};
type CreationJSON = Omit<
  PublicKeyCredentialCreationOptions,
  "challenge" | "user" | "excludeCredentials"
> & {
  challenge: string;
  user: Omit<PublicKeyCredentialUserEntity, "id"> & { id: string };
  excludeCredentials?: (Omit<PublicKeyCredentialDescriptor, "id"> & {
    id: string;
  })[];
};
type RequestJSON = Omit<
  PublicKeyCredentialRequestOptions,
  "challenge" | "allowCredentials"
> & {
  challenge: string;
  allowCredentials?: (Omit<PublicKeyCredentialDescriptor, "id"> & {
    id: string;
  })[];
};
function decode(value: string): ArrayBuffer {
  const base = value.replace(/-/g, "+").replace(/_/g, "/");
  return Uint8Array.from(
    atob(base.padEnd(Math.ceil(base.length / 4) * 4, "=")),
    (c) => c.charCodeAt(0),
  ).buffer;
}
function encode(value: ArrayBuffer) {
  return btoa(
    Array.from(new Uint8Array(value), (c) => String.fromCharCode(c)).join(""),
  )
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}
function serialize(credential: PublicKeyCredential): CredentialJSON {
  const response = credential.response;
  const common = { clientDataJSON: encode(response.clientDataJSON) };
  let data: Record<string, unknown>;
  if (response instanceof AuthenticatorAttestationResponse) {
    data = {
      ...common,
      attestationObject: encode(response.attestationObject),
      transports: response.getTransports?.() ?? [],
    };
  } else {
    const assertion = response as AuthenticatorAssertionResponse;
    data = {
      ...common,
      authenticatorData: encode(assertion.authenticatorData),
      signature: encode(assertion.signature),
      userHandle: assertion.userHandle ? encode(assertion.userHandle) : null,
    };
  }
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: data,
    extensions: credential.getClientExtensionResults(),
  };
}
async function post<T>(path: string, body?: unknown) {
  return api<T>(path, {
    method: "POST",
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}
export async function registerPasskey(body: unknown) {
  const start = await post<Challenge>(
    "/api/v1/me/passkeys/register/start",
    body,
  );
  const options = start.options.publicKey as CreationJSON;
  const credential = await navigator.credentials.create({
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      user: { ...options.user, id: decode(options.user.id) },
      excludeCredentials: options.excludeCredentials?.map((c) => ({
        ...c,
        id: decode(c.id),
      })),
    },
  });
  if (!(credential instanceof PublicKeyCredential))
    throw new Error("Passkey registration was cancelled");
  await post("/api/v1/me/passkeys/register/finish", {
    challenge_id: start.challenge_id,
    credential: serialize(credential),
  });
}
async function assertion(start: Challenge) {
  const options = start.options.publicKey as RequestJSON;
  const credential = await navigator.credentials.get({
    publicKey: {
      ...options,
      challenge: decode(options.challenge),
      allowCredentials: options.allowCredentials?.map((c) => ({
        ...c,
        id: decode(c.id),
      })),
    },
  });
  if (!(credential instanceof PublicKeyCredential))
    throw new Error("Passkey verification was cancelled");
  return {
    challenge_id: start.challenge_id,
    credential: serialize(credential),
  };
}
export async function verifyPasskey(email: string, password: string) {
  const start = await post<Challenge>("/api/v1/auth/passkeys/verify/start", {
    email,
    password,
  });
  return post<{ mfa_proof: string }>(
    "/api/v1/auth/passkeys/verify/finish",
    await assertion(start),
  );
}
export async function signInPasskey() {
  const start = await post<Challenge>("/api/v1/auth/passkeys/login/start");
  return post<SessionResponse>(
    "/api/v1/auth/passkeys/login/finish",
    await assertion(start),
  );
}
