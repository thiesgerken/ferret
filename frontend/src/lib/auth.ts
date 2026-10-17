import { SESSION_ENDED_EVENT } from "@/lib/graphql";

export async function login(username: string, password: string): Promise<void> {
  const res = await fetch("/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    throw new Error(await errorMessage(res, "Sign-in failed"));
  }
}

export async function logout(): Promise<void> {
  const res = await fetch("/auth/logout", { method: "POST" });
  if (!res.ok) {
    throw new Error(await errorMessage(res, "Sign-out failed"));
  }
}

/** Changes the signed-in user's password. The server ends every other session and re-issues this
 * one's cookie. */
export async function changePassword(currentPassword: string, newPassword: string): Promise<void> {
  const res = await fetch("/auth/password", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
  });
  if (res.status === 401) {
    window.dispatchEvent(new Event(SESSION_ENDED_EVENT));
  }
  if (!res.ok) {
    throw new Error(await errorMessage(res, "Changing the password failed"));
  }
}

async function errorMessage(res: Response, fallback: string): Promise<string> {
  const body: unknown = await res.json().catch(() => null);
  if (typeof body === "object" && body !== null && "error" in body && typeof body.error === "string") {
    return body.error;
  }
  return `${fallback} (HTTP ${res.status})`;
}
