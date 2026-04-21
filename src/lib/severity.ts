export function severityIcon(sev: string): string {
  switch (sev) {
    case "error":
      return "✕";
    case "warning":
      return "⚠";
    case "info":
      return "ℹ";
    default:
      return "◦";
  }
}
