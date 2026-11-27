import * as Crypto from "expo-crypto";
import * as WebBrowser from "expo-web-browser";
import { deleteItem, getItem, setItem } from "./storage";
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

WebBrowser.maybeCompleteAuthSession();

const REDIRECT_URI = "rudder://oauth/callback";
const CLIENT_NAME = "Rudder iOS";

interface StoredSession {
  panelUrl: string;
  clientId: string;
  accessToken: string;
  refreshToken: string;
  expiresAt: number;
}

export interface ApiErrorShape {
  status: number;
  code: string;
  message: string;
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

interface AuthState {
  loading: boolean;
  session: StoredSession | null;
  panelUrl: string | null;
  signIn: (panelUrl: string) => Promise<void>;
  signInWithToken: (panelUrl: string, token: string) => Promise<void>;
  signOut: () => Promise<void>;
  api: <T>(path: string, init?: RequestInit) => Promise<T>;
}

const AuthContext = createContext<AuthState | null>(null);

function storeKey(panelUrl: string) {
  return `rudder_session_${panelUrl.replace(/[^a-zA-Z0-9]/g, "_")}`;
}

export function normalizePanelUrl(input: string): string {
  let url = input.trim().replace(/\/+$/, "");
  if (!/^https?:\/\//i.test(url)) {
    url = `https://${url}`;
  }
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new ApiError(0, "invalid_panel_url", "Địa chỉ panel không hợp lệ.");
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new ApiError(0, "invalid_panel_url", "Địa chỉ panel không được chứa credentials, query hoặc fragment.");
  }
  if (parsed.protocol === "http:" && !isLoopbackHostname(parsed.hostname)) {
    throw new ApiError(
      0,
      "https_required",
      "Panel phải dùng HTTPS trên thiết bị di động; HTTP chỉ được chấp nhận cho localhost khi phát triển.",
    );
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    throw new ApiError(0, "invalid_panel_url", "Địa chỉ panel phải bắt đầu bằng https://.");
  }
  return url;
}

function isLoopbackHostname(hostname: string): boolean {
  const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, "");
  return normalized === "localhost" || normalized === "127.0.0.1" || normalized === "::1";
}

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function base64url(bytes: Uint8Array): string {
  let output = "";
  for (let index = 0; index < bytes.length; index += 3) {
    const a = bytes[index];
    const b = bytes[index + 1];
    const c = bytes[index + 2];
    output += B64[a >> 2];
    output += B64[((a & 3) << 4) | (index + 1 < bytes.length ? b >> 4 : 0)];
    output += index + 1 < bytes.length ? B64[((b & 15) << 2) | (index + 2 < bytes.length ? c >> 6 : 0)] : "";
    output += index + 2 < bytes.length ? B64[c & 63] : "";
  }
  return output.replace(/\+/g, "-").replace(/\//g, "_");
}

async function pkcePair(): Promise<{ verifier: string; challenge: string }> {
  const random = await Crypto.getRandomBytesAsync(48);
  const verifier = base64url(random);
  const digest = await Crypto.digestStringAsync(Crypto.CryptoDigestAlgorithm.SHA256, verifier, {
    encoding: Crypto.CryptoEncoding.BASE64,
  });
  const challenge = digest.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return { verifier, challenge };
}

async function postForm<T>(url: string, body: Record<string, unknown>): Promise<T> {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  let data: Record<string, unknown> = {};
  try {
    data = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  } catch {
    // fall through to error handling
  }
  if (!response.ok) {
    const message =
      (data.error_description as string) ?? (data.error as string) ?? response.statusText;
    throw new ApiError(response.status, (data.error as string) ?? "error", message);
  }
  return data as T;
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [loading, setLoading] = useState(true);
  const [session, setSession] = useState<StoredSession | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const last = await getItem("rudder_last_panel");
        if (last) {
          const raw = await getItem(storeKey(last));
          if (raw) {
            setSession(JSON.parse(raw) as StoredSession);
          }
        }
      } catch {
        // no stored session
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  async function persist(next: StoredSession | null) {
    setSession(next);
    if (next) {
      await setItem(storeKey(next.panelUrl), JSON.stringify(next));
      await setItem("rudder_last_panel", next.panelUrl);
    } else if (session) {
      await deleteItem(storeKey(session.panelUrl));
      await deleteItem("rudder_last_panel");
    }
  }

  async function registerClient(panelUrl: string): Promise<string> {
    const key = `rudder_client_${panelUrl.replace(/[^a-zA-Z0-9]/g, "_")}`;
    const cached = await getItem(key);
    if (cached) {
      return cached;
    }
    const registered = await postForm<{ client_id: string }>(`${panelUrl}/oauth/register`, {
      client_name: CLIENT_NAME,
      redirect_uris: [REDIRECT_URI],
      token_endpoint_auth_method: "none",
    });
    await setItem(key, registered.client_id);
    return registered.client_id;
  }

  async function exchange(
    panelUrl: string,
    body: Record<string, string>,
  ): Promise<Omit<StoredSession, "panelUrl" | "clientId">> {
    const token = await postForm<{
      access_token: string;
      refresh_token: string;
      expires_in: number;
    }>(`${panelUrl}/oauth/token`, body);
    return {
      accessToken: token.access_token,
      refreshToken: token.refresh_token,
      expiresAt: Date.now() + token.expires_in * 1000 - 15_000,
    };
  }

  async function signIn(panelUrlRaw: string) {
    const panelUrl = normalizePanelUrl(panelUrlRaw);
    const clientId = await registerClient(panelUrl);
    const { verifier, challenge } = await pkcePair();
    const state = base64url(await Crypto.getRandomBytesAsync(16));
    const params = new URLSearchParams({
      response_type: "code",
      client_id: clientId,
      redirect_uri: REDIRECT_URI,
      code_challenge: challenge,
      code_challenge_method: "S256",
      state,
    });
    const result = await WebBrowser.openAuthSessionAsync(
      `${panelUrl}/oauth/authorize?${params.toString()}`,
      REDIRECT_URI,
    );
    if (result.type !== "success") {
      throw new ApiError(0, "cancelled", "Đăng nhập đã bị hủy.");
    }
    const returned = new URL(result.url);
    const code = returned.searchParams.get("code");
    if (!code || returned.searchParams.get("state") !== state) {
      throw new ApiError(0, "invalid_callback", "Panel trả về phản hồi đăng nhập không hợp lệ.");
    }
    const tokens = await exchange(panelUrl, {
      grant_type: "authorization_code",
      code,
      redirect_uri: REDIRECT_URI,
      client_id: clientId,
      code_verifier: verifier,
    });
    await persist({ panelUrl, clientId, ...tokens });
  }

  async function signInWithToken(panelUrlRaw: string, token: string) {
    const panelUrl = normalizePanelUrl(panelUrlRaw);
    const trimmed = token.trim();
    if (!trimmed) {
      throw new ApiError(0, "empty_token", "Token trống.");
    }
    const response = await fetch(`${panelUrl}/mcp/api/v1/me`, {
      headers: { authorization: `Bearer ${trimmed}` },
    });
    if (!response.ok) {
      throw new ApiError(response.status, "invalid_token", "Token không được panel chấp nhận.");
    }
    await persist({
      panelUrl,
      clientId: "",
      accessToken: trimmed,
      refreshToken: "",
      expiresAt: Number.MAX_SAFE_INTEGER,
    });
  }

  async function signOut() {
    const current = session;
    if (current?.refreshToken) {
      try {
        await postForm(`${current.panelUrl}/oauth/revoke`, {
          token: current.refreshToken,
          client_id: current.clientId,
        });
      } catch {
        // best effort
      }
    }
    await persist(null);
  }

  async function freshToken(): Promise<StoredSession> {
    if (!session) {
      throw new ApiError(401, "unauthorized", "Chưa đăng nhập.");
    }
    if (!session.refreshToken || Date.now() < session.expiresAt) {
      return session;
    }
    try {
      const tokens = await exchange(session.panelUrl, {
        grant_type: "refresh_token",
        refresh_token: session.refreshToken,
        client_id: session.clientId,
      });
      const next: StoredSession = { ...session, ...tokens };
      await persist(next);
      return next;
    } catch (error) {
      if (error instanceof ApiError && (error.status === 400 || error.status === 401)) {
        await persist(null);
        throw new ApiError(401, "session_expired", "Phiên đăng nhập hết hạn, đăng nhập lại.");
      }
      throw error;
    }
  }

  async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
    const active = await freshToken();
    const headers = new Headers(init.headers);
    headers.set("authorization", `Bearer ${active.accessToken}`);
    if (init.body && !headers.has("content-type")) {
      headers.set("content-type", "application/json");
    }
    // `/mcp/api/v1/...` is the same API mirrored under the `/mcp` prefix, which
    // Cloudflare-protected panels expose to machine credentials at the edge.
    const machinePath = path.startsWith("/api/") ? `/mcp${path}` : path;
    const response = await fetch(`${active.panelUrl}${machinePath}`, { ...init, headers });
    if (response.status === 204) {
      return undefined as T;
    }
    const text = await response.text();
    let data: unknown = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = null;
    }
    if (!response.ok) {
      const payload = data as { error?: { code?: string; message?: string } } | null;
      throw new ApiError(
        response.status,
        payload?.error?.code ?? "error",
        payload?.error?.message ?? response.statusText,
      );
    }
    return data as T;
  }

  const value = useMemo<AuthState>(
    () => ({
      loading,
      session,
      panelUrl: session?.panelUrl ?? null,
      signIn,
      signInWithToken,
      signOut,
      api,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [loading, session],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  const value = useContext(AuthContext);
  if (!value) {
    throw new Error("AuthProvider missing");
  }
  return value;
}
