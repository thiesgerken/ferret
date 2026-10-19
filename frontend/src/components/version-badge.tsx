import { AlertTriangle } from "lucide-react";
import { useTranslations } from "next-intl";
import { version as frontendVersion } from "../../package.json";

export function VersionBadge({ serverVersion }: { serverVersion: string }) {
  const t = useTranslations("app.version");
  const mismatch = frontendVersion !== serverVersion;
  const tooltip = [
    t("frontend", { version: frontendVersion }),
    t("server", { version: serverVersion }),
    ...(mismatch ? ["", t("mismatch")] : []),
  ].join("\n");

  return (
    <span title={tooltip} className="flex min-w-0 cursor-help items-center gap-1">
      <span className="truncate">v{serverVersion}</span>
      {mismatch && (
        <>
          <AlertTriangle className="size-3.5 shrink-0 text-amber-600 dark:text-amber-400" aria-hidden />
          <span className="sr-only">{t("mismatch")}</span>
        </>
      )}
    </span>
  );
}
