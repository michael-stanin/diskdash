import { api, ApiError, getToken, resetTokenCache } from "./client";

function setMeta(content: string) {
  document.head.innerHTML = `<meta name="diskdash-token" content="${content}">`;
}

function mockFetch(impl: (url: string, init?: RequestInit) => Response) {
  const fn = vi.fn((url: string, init?: RequestInit) => Promise.resolve(impl(url, init)));
  vi.stubGlobal("fetch", fn);
  return fn;
}

const json = (v: unknown, status = 200) =>
  new Response(JSON.stringify(v), { status, headers: { "Content-Type": "application/json" } });

describe("api client", () => {
  beforeEach(() => {
    resetTokenCache();
    vi.unstubAllGlobals();
  });

  it("reads the token from the injected meta tag", async () => {
    setMeta("abc123");
    await expect(getToken()).resolves.toBe("abc123");
  });

  it("falls back to /session when the placeholder is still present (dev)", async () => {
    setMeta("__DISKDASH_TOKEN__");
    const f = mockFetch(() => json({ token: "dev-token" }));
    await expect(getToken()).resolves.toBe("dev-token");
    expect(f).toHaveBeenCalledWith("/api/v1/session");
  });

  it("attaches the token only to state-changing requests", async () => {
    setMeta("tok");
    const f = mockFetch(() => json({ ok: true }));
    await api("/health");
    await api("/settings", { method: "PUT", body: { a: 1 } });
    const getHeaders = f.mock.calls[0]![1]!.headers as Record<string, string>;
    const putHeaders = f.mock.calls[1]![1]!.headers as Record<string, string>;
    expect(getHeaders["X-Diskdash-Token"]).toBeUndefined();
    expect(putHeaders["X-Diskdash-Token"]).toBe("tok");
    expect(putHeaders["Content-Type"]).toBe("application/json");
  });

  it("turns error bodies into ApiError with field messages", async () => {
    setMeta("tok");
    mockFetch(() =>
      json({ error: "validation", message: "validation failed", fields: { stale_months: "too big" } }, 422),
    );
    const err = await api("/settings", { method: "PUT", body: {} }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).status).toBe(422);
    expect((err as ApiError).fields).toEqual({ stale_months: "too big" });
  });
});
