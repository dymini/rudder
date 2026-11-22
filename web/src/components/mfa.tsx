import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { api } from "../api";
import { useT } from "../i18n";
import { registerPasskey, verifyPasskey } from "../passkeys";
import { Button, Card, Field, TextInput } from "./ui";
import type { Schemas } from "../types";

export type Reauth = {
  password: string;
  totp_code?: string;
  email_code?: string;
  mfa_proof?: string;
};
export const clearFactor = (value: Reauth): Reauth => ({
  password: value.password,
});

export function MfaFields({
  email,
  value,
  onChange,
}: {
  email: string;
  value: Reauth;
  onChange: (value: Reauth) => void;
}) {
  const t = useT();
  const [method, setMethod] = useState("totp");
  const [pending, setPending] = useState(false);
  async function verify() {
    setPending(true);
    try {
      onChange({
        password: value.password,
        ...(await verifyPasskey(email, value.password)),
      });
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t("Passkey operation failed"),
      );
    } finally {
      setPending(false);
    }
  }
  async function send() {
    setPending(true);
    try {
      await api("/api/v1/me/mfa/email/send", {
        method: "POST",
        body: JSON.stringify({ password: value.password, purpose: "reauth" }),
      });
      toast.success(t("Email code sent"));
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t("Could not send email code"),
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="grid max-w-md gap-3">
      <Field label={t("Current password")}>
        <TextInput
          type="password"
          autoComplete="current-password"
          value={value.password}
          onChange={(e) =>
            onChange({ ...clearFactor(value), password: e.target.value })
          }
        />
      </Field>
      <p className="text-sm text-muted">
        {t(
          "Security changes require your password and an enabled factor. No factor is needed before your first enrollment.",
        )}
      </p>
      <label className="grid gap-1 text-sm">
        {t("Verification method")}
        <select
          className="min-h-10 rounded-control border border-line bg-surface px-3"
          value={method}
          onChange={(e) => {
            setMethod(e.target.value);
            onChange(clearFactor(value));
          }}
        >
          <option value="totp">{t("Authenticator or recovery code")}</option>
          <option value="email">{t("Email code")}</option>
          <option value="passkey">{t("Passkey")}</option>
        </select>
      </label>
      {method === "passkey" ? (
        <Button
          type="button"
          quiet
          disabled={
            pending ||
            !value.password ||
            !window.PublicKeyCredential ||
            !!value.mfa_proof
          }
          onClick={() => void verify()}
        >
          {value.mfa_proof ? t("Passkey verified") : t("Verify with passkey")}
        </Button>
      ) : (
        <>
          <Field label={t("Current code")}>
            <TextInput
              autoComplete="one-time-code"
              value={
                method === "email"
                  ? (value.email_code ?? "")
                  : (value.totp_code ?? "")
              }
              onChange={(e) =>
                onChange({
                  ...clearFactor(value),
                  [method === "email" ? "email_code" : "totp_code"]:
                    e.target.value || undefined,
                })
              }
            />
          </Field>
          {method === "email" ? (
            <Button
              type="button"
              quiet
              disabled={pending || !value.password}
              onClick={() => void send()}
            >
              {t("Send email code")}
            </Button>
          ) : null}
        </>
      )}
    </div>
  );
}

export function MfaSecurity({
  reauth,
  setReauth,
}: {
  reauth: Reauth;
  setReauth: (value: Reauth) => void;
}) {
  const t = useT();
  const query = useQuery({
    queryKey: ["mfa-methods"],
    queryFn: () => api<Schemas["MfaMethods"]>("/api/v1/me/mfa"),
  });
  const keyQuery = useQuery({
    queryKey: ["passkeys"],
    queryFn: () => api<Schemas["PasskeyView"][]>("/api/v1/me/passkeys"),
  });
  const [name, setName] = useState("");
  const [code, setCode] = useState("");
  const [setup, setSetup] = useState(false);
  const [pending, setPending] = useState(false);
  async function run(action: () => Promise<unknown>) {
    setPending(true);
    try {
      await action();
      await Promise.all([query.refetch(), keyQuery.refetch()]);
      toast.success(t("Security settings updated"));
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : t("Could not update security settings"),
      );
    } finally {
      setPending(false);
      setReauth(clearFactor(reauth));
    }
  }
  const methods = query.data;
  return (
    <>
      <Card
        title={t("Passkeys")}
        description={t(
          "Use a security key or your device lock to verify sign-in. Keep another method in case a device is lost.",
        )}
      >
        {keyQuery.isError || query.isError ? (
          <p role="alert" className="break-words text-danger">
            {t("Could not load MFA methods")}
          </p>
        ) : null}
        <p className="break-all text-xs text-muted">
          {methods?.passkey_origin ??
            t("Configure an HTTPS panel domain or public URL to use passkeys.")}
        </p>
        <div className="mt-4 space-y-3">
          {keyQuery.data?.map((key) => (
            <PasskeyRow
              key={key.id}
              item={key}
              pending={pending}
              run={run}
              reauth={reauth}
            />
          ))}
        </div>
        {keyQuery.data?.length === 0 ? (
          <p className="text-sm text-muted">{t("No passkeys registered")}</p>
        ) : null}
        <form
          className="mt-4 flex flex-wrap items-end gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void run(async () => {
              await registerPasskey({ ...reauth, name });
              setName("");
            });
          }}
        >
          <Field label={t("Passkey name")}>
            <TextInput
              required
              maxLength={100}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Button
            type="submit"
            disabled={
              pending ||
              !reauth.password ||
              !methods?.passkey_origin ||
              !window.PublicKeyCredential
            }
          >
            {t("Add passkey")}
          </Button>
        </form>
      </Card>
      <Card title={t("Email verification")}>
        <p className="text-sm text-muted">
          {t(
            "Codes expire in 10 minutes and can be tried five times. Email delivery is limited to once per minute.",
          )}
        </p>
        {!methods?.email_available ? (
          <p className="text-sm text-muted">
            {t(
              "Ask an administrator to configure an email provider in Access settings.",
            )}
          </p>
        ) : null}
        <div className="mt-3 flex flex-wrap gap-3">
          {methods?.email ? (
            <Button
              quiet
              disabled={pending || !reauth.password}
              onClick={() =>
                void run(() =>
                  api("/api/v1/me/mfa/email/disable", {
                    method: "POST",
                    body: JSON.stringify(reauth),
                  }),
                )
              }
            >
              {t("Disable email verification")}
            </Button>
          ) : (
            <Button
              disabled={
                pending || !reauth.password || !methods?.email_available
              }
              onClick={() =>
                void run(async () => {
                  await api("/api/v1/me/mfa/email/send", {
                    method: "POST",
                    body: JSON.stringify({ ...reauth, purpose: "enable" }),
                  });
                  setSetup(true);
                })
              }
            >
              {t("Send enrollment code")}
            </Button>
          )}
        </div>
        {setup && !methods?.email ? (
          <form
            className="mt-4 grid max-w-md gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              void run(async () => {
                await api("/api/v1/me/mfa/email/enable", {
                  method: "POST",
                  body: JSON.stringify({ password: reauth.password, code }),
                });
                setSetup(false);
                setCode("");
              });
            }}
          >
            <Field label={t("Email code")}>
              <TextInput
                required
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                value={code}
                onChange={(e) => setCode(e.target.value)}
              />
            </Field>
            <Button disabled={pending}>{t("Enable email verification")}</Button>
          </form>
        ) : null}
      </Card>
    </>
  );
}
function PasskeyRow({
  item,
  pending,
  run,
  reauth,
}: {
  item: Schemas["PasskeyView"];
  pending: boolean;
  run: (action: () => Promise<unknown>) => Promise<void>;
  reauth: Reauth;
}) {
  const t = useT();
  const [name, setName] = useState(item.name);
  return (
    <div className="flex min-w-0 flex-wrap items-end gap-2 rounded-control border border-line p-3">
      <div className="min-w-0 flex-1">
        <TextInput
          aria-label={t("Passkey name")}
          maxLength={100}
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <p className="mt-1 break-words text-xs text-muted">
          {t("Last used")}:{" "}
          {item.last_used_at
            ? new Date(item.last_used_at).toLocaleString()
            : "—"}
        </p>
      </div>
      <Button
        quiet
        disabled={pending || !name.trim()}
        onClick={() =>
          void run(() =>
            api(`/api/v1/me/passkeys/${item.id}`, {
              method: "PATCH",
              body: JSON.stringify({ name }),
            }),
          )
        }
      >
        {t("Rename")}
      </Button>
      <Button
        variant="danger"
        disabled={pending || !reauth.password}
        onClick={() => {
          if (
            window.confirm(
              t(
                "Delete this passkey? You will need another method if this device is lost.",
              ),
            )
          )
            void run(() =>
              api(`/api/v1/me/passkeys/${item.id}`, {
                method: "DELETE",
                body: JSON.stringify(reauth),
              }),
            );
        }}
      >
        {t("Delete")}
      </Button>
    </div>
  );
}
