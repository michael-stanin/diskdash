import type { ApiErrorBody } from "./types/ApiErrorBody";

const TOKEN_PLACEHOLDER = "__DISKDASH_TOKEN__";
const TOKEN_HEADER = "X-Diskdash-Token";
const UNSAFE = new Set(["POST", "PUT", "PATCH", "DELETE"]);

export class ApiError extends Error {
  readonly status: number;
  readonly body: ApiErrorBody | null;

  constructor(status: number, body: ApiErrorBody | null, fallback: string) {
    super(body?.message ?? fallback);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }

  /** Field-level validation messages (422). */
  get fields(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(this.body?.fields ?? {})) if (v !== undefined) out[k] = v;
    return out;
  }
}

let tokenPromise: Promise<string> | null = null;

/**
 * The CSRF token is injected into index.html by the server. Under the Vite dev
 * server the placeholder is still there, so ask the backend (dev mode only).
 */
export function getToken(doc: Document = document): Promise<string> {
  if (tokenPromise) return tokenPromise;
  const meta = doc.querySelector<HTMLMetaElement>('meta[name="diskdash-token"]')?.content ?? "";
  if (meta && meta !== TOKEN_PLACEHOLDER) {
    tokenPromise = Promise.resolve(meta);
  } else {
    tokenPromise = fetch("/api/v1/session")
      .then((r) => (r.ok ? (r.json() as Promise<{ token: string }>) : Promise.reject(new Error("no session"))))
      .then((s) => s.token)
      .catch((e: unknown) => {
        tokenPromise = null;
        throw e;
      });
  }
  return tokenPromise;
}

/** Test hook. */
export function resetTokenCache(): void {
  tokenPromise = null;
}

export interface RequestOptions {
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  body?: unknown;
  signal?: AbortSignal;
}

export async function api<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const method = opts.method ?? "GET";
  const headers: Record<string, string> = { Accept: "application/json" };
  if (opts.body !== undefined) headers["Content-Type"] = "application/json";
  if (UNSAFE.has(method)) headers[TOKEN_HEADER] = await getToken();

  const res = await fetch(`/api/v1${path}`, {
    method,
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
    signal: opts.signal,
    credentials: "same-origin",
  });
  if (!res.ok) {
    let body: ApiErrorBody | null = null;
    try {
      body = (await res.json()) as ApiErrorBody;
    } catch {
      // Non-JSON error body.
    }
    throw new ApiError(res.status, body, `${method} ${path} failed with ${res.status}`);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** POST a JSON body and return the response as a file (reports). The name comes from Content-Disposition. */
export async function apiDownload(path: string, body: unknown): Promise<{ blob: Blob; fileName: string }> {
  const res = await fetch(`/api/v1${path}`, {
    method: "POST",
    headers: { Accept: "*/*", "Content-Type": "application/json", [TOKEN_HEADER]: await getToken() },
    body: JSON.stringify(body),
    credentials: "same-origin",
  });
  if (!res.ok) {
    let parsed: ApiErrorBody | null = null;
    try {
      parsed = (await res.json()) as ApiErrorBody;
    } catch {
      // Non-JSON error body.
    }
    throw new ApiError(res.status, parsed, `POST ${path} failed with ${res.status}`);
  }
  const disposition = res.headers.get("Content-Disposition") ?? "";
  const fileName = /filename="?([^";]+)"?/.exec(disposition)?.[1] ?? "diskdash-report";
  return { blob: await res.blob(), fileName };
}
