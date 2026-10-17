import { CircleDashed } from "lucide-react";
import { NamedIcon } from "@/components/named-icon";
import { sourceLabel, type Writer } from "@/lib/source-label";

/** A source under its writer's icon and name, the full path in the tooltip. */
export function SourceName({
  source,
  writer,
  roots,
}: {
  source: string;
  writer: Writer | null | undefined;
  roots: Map<string, string> | undefined;
}) {
  const label = sourceLabel(source, writer, roots);
  return (
    <span title={source} className="inline-flex min-w-0 items-center gap-1.5">
      {label.icon ? (
        <NamedIcon name={label.icon} className="size-3.5 shrink-0" />
      ) : (
        <CircleDashed className="size-3.5 shrink-0" aria-hidden />
      )}
      <span className="truncate">{label.text}</span>
    </span>
  );
}
