// TS twin of core's URL password redactor. The UI must never echo broker
// credentials back in plaintext — always render URLs through this.

const SCHEME_RE = /^(?:mqtt|mqtts|ws|wss):\/\//i;

/**
 * Replace the password in a `mqtt://`, `mqtts://`, `ws://` or `wss://` URL
 * with `••••`: `mqtt://user:pass@host` → `mqtt://user:••••@host`.
 *
 * No-op when the URL has no recognized scheme, no userinfo (`@`), or no
 * password (no `:` inside the userinfo). Only the first `:` in the userinfo
 * delimits the password, so passwords containing `:` `@` etc. are fully
 * redacted.
 */
export function redactPassword(url: string): string {
  const m = SCHEME_RE.exec(url);
  if (!m) return url;
  const schemeEnd = m[0].length;
  const pathStart = url.indexOf("/", schemeEnd);
  const authority =
    pathStart === -1 ? url.slice(schemeEnd) : url.slice(schemeEnd, pathStart);
  const at = authority.lastIndexOf("@");
  if (at === -1) return url; // no auth section
  const userinfo = authority.slice(0, at);
  const colon = userinfo.indexOf(":");
  if (colon === -1) return url; // user only, no password
  return (
    url.slice(0, schemeEnd) +
    userinfo.slice(0, colon + 1) +
    "••••" +
    authority.slice(at) +
    (pathStart === -1 ? "" : url.slice(pathStart))
  );
}
