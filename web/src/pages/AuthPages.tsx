import { useState, type FormEvent } from "react";
import { Navigate, useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import { api, ApiError } from "../api";
import { rememberSession, useAuth } from "../auth";
import { Icon, KnotreeMark } from "../components/icons";
import { InsecureBanner } from "../components/insecure-banner";
import { Button, Field, TextInput } from "../components/ui";
import { LANGUAGES, useLanguage, useT } from "../i18n";
import type { SessionResponse } from "../types";
import { signInPasskey, verifyPasskey } from "../passkeys";

export function SetupPage() {
  const t = useT();
  const { needsSetup, loading, refresh, user } = useAuth();
  const [params] = useSearchParams();
  const [setupToken, setSetupToken] = useState(() => params.get("token") ?? "");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);

  if (!loading && user) {
    return <Navigate to="/" replace />;
  }
  if (!loading && !needsSetup) {
    return <Navigate to="/login" replace />;
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setPending(true);
    try {
      const body = await api<SessionResponse>("/api/v1/setup", {
        method: "POST",
        body: JSON.stringify({
          setup_token: setupToken.trim(),
          email,
          password,
          display_name: displayName || undefined,
        }),
      });
      rememberSession(body);
      await refresh();
    } catch (error) {
      toast.error(
        error instanceof ApiError ? error.message : t("Setup failed"),
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <AuthFrame
      title={t("Create the owner")}
      subtitle={t("This account administers the panel.")}
    >
      <form className="grid gap-4" onSubmit={(event) => void onSubmit(event)}>
        <Field
          label={t("Setup token")}
          hint={t(
            "The installer prints it. It is also in setup-token in the panel's data directory.",
          )}
        >
          <TextInput
            required
            autoComplete="off"
            spellCheck={false}
            value={setupToken}
            onChange={(event) => setSetupToken(event.target.value)}
          />
        </Field>
        <Field label={t("Email")}>
          <TextInput
            type="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
        </Field>
        <Field label={t("Display name")}>
          <TextInput
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
          />
        </Field>
        <Field
          label={t("Password")}
          hint={t("Use at least 12 characters. A short phrase works well.")}
        >
          <TextInput
            type="password"
            required
            minLength={12}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </Field>
        <Button type="submit" size="lg" disabled={pending} className="mt-2 w-full">
          {pending ? t("Creating…") : t("Create owner")}
        </Button>
      </form>
    </AuthFrame>
  );
}

/** Only the agent approval flow may leave the sign-in page for a specific path. */
function safeNext(value: string | null): string | null {
  if (
    !value ||
    !value.startsWith("/") ||
    value.startsWith("//") ||
    value.includes("\\")
  ) {
    return null;
  }
  const path = value.split("?")[0];
  if (
    path === "/oauth/authorize" ||
    path === "/mcp/consent" ||
    path === "/tokens"
  ) {
    return value;
  }
  return null;
}

export function LoginPage() {
  const t = useT();
  const { needsSetup, loading, refresh, user } = useAuth();
  const [params] = useSearchParams();
  const next = safeNext(params.get("next"));
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [totp, setTotp] = useState("");
  const [needsTotp, setNeedsTotp] = useState(false);
  const [factor, setFactor] = useState("totp");
  const [pending, setPending] = useState(false);

  if (!loading && needsSetup) {
    return <Navigate to="/setup" replace />;
  }
  if (!loading && user) {
    return <Navigate to={next ?? "/"} replace />;
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    await signIn();
  }

  async function signIn(mfa_proof?: string) {
    setPending(true);
    try {
      const body = await api<SessionResponse>("/api/v1/login", {
        method: "POST",
        body: JSON.stringify({
          email,
          password,
          totp_code: factor === "totp" ? totp || undefined : undefined,
          email_code: factor === "email" ? totp || undefined : undefined,
          mfa_proof,
        }),
      });
      rememberSession(body);
      await refresh();
    } catch (error) {
      if (
        error instanceof ApiError &&
        ["totp_required", "mfa_required"].includes(error.code)
      ) {
        setNeedsTotp(true);
        toast.message(t("Use an enabled MFA method"));
      } else {
        toast.error(
          error instanceof ApiError ? error.message : t("Sign-in failed"),
        );
      }
    } finally {
      setPending(false);
    }
  }

  async function passkey(discoverable: boolean) {
    setPending(true);
    try {
      if (discoverable) {
        rememberSession(await signInPasskey());
        await refresh();
      } else {
        const proof = await verifyPasskey(email, password);
        await signIn(proof.mfa_proof);
      }
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t("Passkey operation failed"),
      );
    } finally {
      setPending(false);
    }
  }

  async function sendEmail() {
    setPending(true);
    try {
      await api("/api/v1/auth/mfa/email/send", {
        method: "POST",
        body: JSON.stringify({ email, password }),
      });
      setFactor("email");
      setTotp("");
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
    <AuthFrame
      title={t("Sign in")}
      subtitle={t("Use your Rudder panel account.")}
    >
      <form className="grid gap-4" onSubmit={(event) => void onSubmit(event)}>
        <Field label={t("Email")}>
          <TextInput
            type="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
        </Field>
        <Field label={t("Password")}>
          <TextInput
            type="password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </Field>
        {needsTotp ? (
          <div className="grid gap-3">
            <p className="text-sm text-muted">
              {t("Use a method enabled for your account.")}
            </p>
            <select
              aria-label={t("Verification method")}
              className="min-h-10 rounded-control border border-line bg-surface px-3"
              value={factor}
              onChange={(e) => {
                setFactor(e.target.value);
                setTotp("");
              }}
            >
              <option value="totp">
                {t("Authenticator or recovery code")}
              </option>
              <option value="email">{t("Email code")}</option>
            </select>
            <Field label={t("Current code")}>
              <TextInput
                inputMode="numeric"
                autoComplete="one-time-code"
                placeholder="000000"
                className="h-12 text-center font-mono text-xl! tracking-[0.35em]"
                value={totp}
                onChange={(event) => setTotp(event.target.value)}
              />
            </Field>
            {factor === "email" ? (
              <Button
                type="button"
                quiet
                disabled={pending}
                onClick={() => void sendEmail()}
              >
                {t("Send email code")}
              </Button>
            ) : null}
            <Button
              type="button"
              quiet
              disabled={pending || !window.PublicKeyCredential}
              onClick={() => void passkey(false)}
            >
              {t("Verify with passkey")}
            </Button>
          </div>
        ) : null}
        <Button
          type="submit"
          size="lg"
          disabled={pending}
          className="mt-2 w-full"
        >
          {pending ? t("Signing in…") : t("Sign in")}
        </Button>
      </form>
      <Button
        type="button"
        quiet
        className="mt-3 w-full"
        disabled={
          pending || !window.PublicKeyCredential || !window.isSecureContext
        }
        onClick={() => void passkey(true)}
      >
        {t("Sign in with passkey")}
      </Button>
    </AuthFrame>
  );
}

function AuthFrame({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: React.ReactNode;
}) {
  const t = useT();
  const { language, setLanguage } = useLanguage();
  return (
    <div className="flex min-h-screen flex-col">
      <InsecureBanner link={false} />
      <header className="flex items-center gap-2.5 px-5 py-5 sm:px-8">
        <KnotreeMark size={26} />
        <span className="text-[15px] font-semibold tracking-[-0.01em] text-ink">Knotree</span>
        <span className="border-l border-line-strong pl-2.5 text-[15px] text-muted">Rudder</span>
      </header>
      <main className="flex flex-1 items-center justify-center px-4 pb-16">
        <div className="w-full max-w-[400px] animate-fade-up rounded-auth border border-line bg-surface p-6 shadow-raised sm:p-8">
          <span className="mb-5 inline-flex size-10 items-center justify-center rounded-alert border border-pine-line bg-pine-soft text-pine">
            <Icon name="key" size={20} />
          </span>
          <h1 className="text-[22px] leading-tight font-semibold tracking-[-0.015em]">{title}</h1>
          <p className="mt-1.5 mb-6 text-[14.5px] text-muted">{subtitle}</p>
          {children}
        </div>
      </main>
      <footer className="flex flex-col items-center gap-2 px-5 py-5 text-center text-xs text-faint sm:px-8">
        <span>{t("Self-hosted deployment panel")}</span>
        <div role="group" aria-label={t("Language")} className="flex items-center gap-1">
          {LANGUAGES.map((item) => (
            <button
              key={item.code}
              type="button"
              aria-pressed={language === item.code}
              onClick={() => setLanguage(item.code)}
              className={`rounded-control px-2 py-1 transition-colors hover:text-ink ${language === item.code ? "bg-sunken font-medium text-ink" : ""}`}
            >
              {item.label}
            </button>
          ))}
        </div>
      </footer>
    </div>
  );
}
