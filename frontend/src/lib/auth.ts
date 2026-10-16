export async function login(username: string, password: string): Promise<void> {
  const res = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    throw new Error(await errorMessage(res, "Sign-in failed"));
  }
}

export async function logout(): Promise<void> {
  const res = await fetch("/api/auth/logout", { method: "POST" });
  if (!res.ok) {
    throw new Error(await errorMessage(res, "Sign-out failed"));
  }
}

async function errorMessage(res: Response, fallback: string): Promise<string> {
  const body: unknown = await res.json().catch(() => null);
  if (typeof body === "object" && body !== null && "error" in body && typeof body.error === "string") {
    return body.error;
  }
  return `${fallback} (HTTP ${res.status})`;
}
