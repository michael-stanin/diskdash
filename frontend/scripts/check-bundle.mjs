// Safety rule 2: the built UI must not reference external hosts (no CDNs,
// web fonts, analytics). Hosts below appear only as inert strings (SVG XML
// namespaces, library error-doc links, a CSS comment) and are never fetched.
// A new host fails the build so a human reviews it.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join } from "node:path";

const ALLOWED_HOSTS = new Set([
  "localhost",
  "127.0.0.1",
  "www.w3.org", // SVG/XLink/XHTML namespaces
  "react.dev", // React error-message links
  "reactrouter.com", // React Router warning links
  "tailwindcss.com", // licence comment in the CSS
]);

const dist = new URL("../dist", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const files = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if ([".js", ".css", ".html", ".svg", ".json"].includes(extname(p))) files.push(p);
  }
})(dist);

const violations = [];
for (const file of files) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/(?:https?:)?\/\/([A-Za-z0-9.-]+\.[A-Za-z]{2,}|localhost|127\.0\.0\.1)(?=[/:"'`)\s?#]|$)/g)) {
    // Require a scheme or a url(/@import/src context for protocol-relative URLs.
    const before = text.slice(Math.max(0, m.index - 8), m.index);
    const hasScheme = m[0].startsWith("http");
    if (!hasScheme && !/(url\(|import |src=|href=)["']?$/.test(before)) continue;
    if (!ALLOWED_HOSTS.has(m[1])) violations.push(`${file}: ${m[0]}`);
  }
}

if (files.length === 0) {
  console.error("check-bundle: dist/ is empty; run `npm run build` first");
  process.exit(1);
}
if (violations.length > 0) {
  console.error("check-bundle: external URLs found in the built UI:");
  for (const v of violations) console.error(`  ${v}`);
  process.exit(1);
}
console.log(`check-bundle: ok (${files.length} files, no external hosts)`);
