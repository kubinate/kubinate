import { parseProblem } from './_problem';

/** `POST /v1/auth/register` — create account + issue session cookie. */
export async function register(
  email: string,
  displayName: string,
  password: string
): Promise<void> {
  const res = await fetch('/api/v1/auth/register', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, display_name: displayName, password })
  });
  if (!res.ok) {
    throw await parseProblem(res, await res.text());
  }
}

/** `POST /v1/auth/login` — verify password + issue session cookie. */
export async function loginPassword(email: string, password: string): Promise<void> {
  const res = await fetch('/api/v1/auth/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, password })
  });
  if (!res.ok) {
    throw await parseProblem(res, await res.text());
  }
}
