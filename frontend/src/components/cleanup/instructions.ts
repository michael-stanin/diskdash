const COMMAND_WORDS = new Set([
  "powercfg",
  "dism",
  "vssadmin",
  "net",
  "npm",
  "pnpm",
  "yarn",
  "pip",
  "dotnet",
  "cargo",
  "python",
  "gradle",
  "optimize-vhd",
  "diskpart",
  "select",
  "attach",
  "compact",
  "detach",
  "wsl",
  "cleanmgr",
  "fsutil",
]);

/**
 * Whether an instruction line is a command the user can copy, as opposed to a
 * sentence. Commands are shown in a code box with a copy button; diskdash
 * never runs them.
 */
export function isCommand(line: string): boolean {
  const t = line.trim();
  if (!t || t.length > 120) return false;
  if (/[.:]$/.test(t) && !/^[A-Za-z]:\\/.test(t)) return false;
  const first = t.split(/\s+/)[0]!.toLowerCase();
  if (COMMAND_WORDS.has(first)) return true;
  if (/^[A-Za-z]:\\/.test(t)) return true; // a path
  if (/^\.venv\\/.test(t)) return true;
  return false;
}
