// 从 apps/desktop/src/desktopModel.ts 复用的纯逻辑子集（该层是纯 TS，
// 不依赖 Tauri）。W2 若把 desktopModel 抽到 packages/ 共享包，这里可以
// 改为直接 import。来源文件保留权威实现。

export type TargetRouteAvailability = {
  available: boolean;
  missing_engines?: readonly string[];
};

export const SUPPORTED_TARGET_FORMATS: readonly string[] = [
  "jpg", "png", "webp", "avif", "mp4", "mp3", "m4a", "wav", "gif", "pdf", "docx", "md", "json", "csv", "yaml", "xml",
];

export function recommendedTargets(path: string): string[] {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  if (["heic", "heif"].includes(extension)) return ["jpg", "png"];
  if (["png", "jpg", "jpeg"].includes(extension)) return ["webp", "avif"];
  if (["mov", "mkv", "avi", "webm"].includes(extension)) return ["mp4", "gif", "mp3"];
  if (["wav", "flac", "aac", "m4a", "ogg", "opus", "mp3"].includes(extension)) {
    return ["m4a", "mp3", "wav"];
  }
  if (extension === "xlsx") return ["pdf", "csv"];
  if (["docx", "pptx"].includes(extension)) return ["pdf"];
  if (["xls", "xlsm", "xlsb"].includes(extension)) return [];
  if (["md", "markdown", "html", "htm"].includes(extension)) return ["pdf", "docx"];
  if (extension === "pdf") return ["png", "jpg", "md"];
  if (["eml", "msg", "mbox"].includes(extension)) return ["md"];
  if (["csv", "json", "yaml", "yml", "xml"].includes(extension)) {
    return ["json", "csv", "yaml", "xml"];
  }
  return [];
}

export function normalizeTargetFormat(target: string): string {
  const normalized = target.trim().replace(/^\./, "").toLowerCase();
  if (normalized === "jpeg") return "jpg";
  if (normalized === "yml") return "yaml";
  return normalized;
}

export type TargetOptionView = { value: string; label: string; disabled: boolean; recommended: boolean };

// HowToConvert 风格选择器：拿到 capabilities 后隐藏此输入用不了的组合，
// 但保留缺引擎路线让用户看见缺口（与桌面版 targetOptionViews 同规则）。
export function targetOptionViews(
  recommendations: readonly string[],
  routes: Readonly<Record<string, TargetRouteAvailability>> | null,
  unavailableLabels: { missing: string; unsupported: string },
): TargetOptionView[] {
  const candidates = Array.from(new Set([...recommendations, ...SUPPORTED_TARGET_FORMATS]));
  const values = routes !== null
    ? candidates.filter(
        (value) =>
          routes[value]?.available === true || (routes[value]?.missing_engines?.length ?? 0) > 0,
      )
    : candidates;
  return values.map((value) => {
    const route = routes?.[value];
    const unavailable = routes !== null && route?.available !== true;
    const reason = (route?.missing_engines?.length ?? 0) > 0
      ? unavailableLabels.missing
      : unavailableLabels.unsupported;
    return {
      value,
      label: unavailable ? `${value} — ${reason}` : value,
      disabled: unavailable,
      recommended: recommendations.includes(value),
    };
  });
}

export type PlainLossBadge = "lossy" | "drop-tracks" | "unknown" | "container" | "lossless";

export function plainLossSummary(plan: {
  steps: ReadonlyArray<{ loss_class: string }>;
  changes: { dropped: readonly string[] };
}): PlainLossBadge {
  const classes = plan.steps.map((step) => step.loss_class.toLowerCase());
  if (classes.includes("lossy")) return "lossy";
  const droppedTracks = plan.changes.dropped.some((item) =>
    /track|stream|subtitle|chapter/i.test(item),
  );
  if (droppedTracks) return "drop-tracks";
  if (classes.includes("unknown")) return "unknown";
  if (classes.every((item) => item === "none" || item === "container-only")) return "container";
  return "lossless";
}

export function pathStemAndExt(input: string): { directory: string; stem: string; ext: string } {
  const separator = Math.max(input.lastIndexOf("/"), input.lastIndexOf("\\"));
  const directory = separator >= 0 ? input.slice(0, separator + 1) : "";
  const filename = separator >= 0 ? input.slice(separator + 1) : input;
  const dot = filename.lastIndexOf(".");
  const stem = dot > 0 ? filename.slice(0, dot) : filename;
  const ext = (dot > 0 ? filename.slice(dot + 1) : "bin").toLowerCase();
  return { directory, stem, ext };
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}
