/** Tokens the file name template understands, for the Settings hint list. */
export const FILENAME_TOKENS = [
  { token: "{date}", example: "2026-11-19" },
  { token: "{time}", example: "20.41.07" },
  { token: "{year}", example: "2026" },
  { token: "{month}", example: "11" },
  { token: "{day}", example: "19" },
  { token: "{hour}", example: "20" },
  { token: "{minute}", example: "41" },
  { token: "{second}", example: "07" },
  { token: "{width}", example: "1440" },
  { token: "{height}", example: "900" },
];

export const DEFAULT_TEMPLATE = "Screenshot-{date}-{time}";

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * Fill in a file name template. Dots separate the time so the name is valid
 * on Windows, which does not allow colons.
 */
export function formatFilename(
  template: string,
  when = new Date(),
  size?: { width: number; height: number },
) {
  const parts: Record<string, string> = {
    year: String(when.getFullYear()),
    month: pad(when.getMonth() + 1),
    day: pad(when.getDate()),
    hour: pad(when.getHours()),
    minute: pad(when.getMinutes()),
    second: pad(when.getSeconds()),
    width: size ? String(size.width) : "",
    height: size ? String(size.height) : "",
  };
  parts.date = `${parts.year}-${parts.month}-${parts.day}`;
  parts.time = `${parts.hour}.${parts.minute}.${parts.second}`;
  const filled = (template.trim() || DEFAULT_TEMPLATE).replace(/\{(\w+)\}/g, (match, key: string) =>
    key in parts ? parts[key] : match,
  );
  return filled.replace(/[/\\:*?"<>|]/g, "-").trim() || "Screenshot";
}
