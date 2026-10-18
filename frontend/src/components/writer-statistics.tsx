"use client";

import { useFormatter, useTranslations } from "next-intl";
import { NamedIcon } from "@/components/named-icon";
import type { ActivityOverviewQuery } from "@/gql/graphql";

type Statistics = ActivityOverviewQuery["writerStatistics"];
const UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"] as const;

export function WriterStatisticsTable({ statistics }: { statistics: Statistics }) {
  const t = useTranslations("activity");
  const format = useFormatter();
  const percentage = (value: number, total: number) => format.number(total === 0 ? 0 : value / total, { style: "percent", maximumFractionDigits: 1 });
  const bytes = (value: number) => {
    const exponent = Math.min(Math.floor(Math.log(Math.max(value, 1)) / Math.log(1024)), UNITS.length - 1);
    return format.number(value / 1024 ** exponent, { style: "unit", unit: UNITS[exponent], maximumFractionDigits: 1 });
  };

  return (
    <div className="space-y-3">
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead className="text-xs text-muted-foreground">
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-4 text-left font-medium">{t("writer")}</th>
              <th scope="col" className="px-3 py-2 text-right font-medium">{t("documents")}</th>
              <th scope="col" className="px-3 py-2 text-right font-medium">{t("chunks")}</th>
              <th scope="col" className="py-2 pl-3 text-right font-medium">{t("estimatedStorage")}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {statistics.rows.map((row) => (
              <tr key={row.writer?.id ?? "unknown"}>
                <th scope="row" className="py-3 pr-4 text-left align-top font-normal">
                  <details>
                    <summary className="cursor-pointer rounded-sm font-medium focus-visible:outline-2 focus-visible:outline-ring">
                      <NamedIcon name={row.writer?.icon ?? "circle-dashed"} className="mr-2 inline size-4 text-muted-foreground" />
                      {row.writer?.name ?? t("unknownWriter")}
                    </summary>
                    <ul aria-label={t("sourcePaths")} className="mt-2 space-y-1 break-all text-xs text-muted-foreground">
                      {row.sources.map((source) => <li key={source}>{source}</li>)}
                    </ul>
                  </details>
                </th>
                <td className="px-3 py-3 text-right align-top tabular-nums">
                  <Count value={format.number(row.documents)} detail={t("documentShare", { percentage: percentage(row.documents, statistics.documents) })} />
                </td>
                <td className="px-3 py-3 text-right align-top tabular-nums">
                  <Count value={format.number(row.chunks)} detail={t("chunkShare", { percentage: percentage(row.chunks, statistics.chunks) })} />
                </td>
                <td className="py-3 pl-3 text-right align-top tabular-nums">
                  <details>
                    <summary className="cursor-pointer rounded-sm whitespace-nowrap focus-visible:outline-2 focus-visible:outline-ring">≈ {bytes(row.storage.total)}</summary>
                    <div className="mt-2 min-w-56 space-y-2 text-xs text-muted-foreground">
                      <p>{t("storageShare", { percentage: percentage(row.storage.total, statistics.storage.total) })}</p>
                      <dl className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 text-left">
                        {(["documents", "chunks", "embeddings", "indexes"] as const).map((part) => (
                          <div key={part} className="contents"><dt>{t(part)}</dt><dd className="text-right">≈ {bytes(row.storage[part])}</dd></div>
                        ))}
                      </dl>
                      {row.models.map((model) => (
                        <div key={model.model} className="border-t border-border pt-2 text-left">
                          <p className="break-all font-medium text-foreground">{model.model}</p>
                          <p>{t("modelStorage", { count: model.embedded, rows: bytes(model.rowBytes), index: bytes(model.indexBytes) })}</p>
                        </div>
                      ))}
                    </div>
                  </details>
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot className="border-t border-border font-medium tabular-nums">
            <tr>
              <th scope="row" className="py-3 pr-4 text-left">{t("yourTotal")}</th>
              <td className="px-3 py-3 text-right">{format.number(statistics.documents)}</td>
              <td className="px-3 py-3 text-right">{format.number(statistics.chunks)}</td>
              <td className="py-3 pl-3 text-right whitespace-nowrap">≈ {bytes(statistics.storage.total)}</td>
            </tr>
          </tfoot>
        </table>
      </div>
      {statistics.rows.length === 0 && <p className="text-sm text-muted-foreground">{t("nothingYet")}</p>}
      <p className="text-xs text-muted-foreground">{t("storageEstimateHint")}</p>
    </div>
  );
}

function Count({ value, detail }: { value: string; detail: string }) {
  return (
    <span tabIndex={0} aria-label={`${value}: ${detail}`} className="group/count relative inline-block cursor-help rounded-sm focus-visible:outline-2 focus-visible:outline-ring">
      {value}
      <span role="tooltip" aria-hidden className="pointer-events-none absolute top-full right-0 z-10 mt-1 hidden w-max max-w-64 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md group-hover/count:block group-focus/count:block">
        {detail}
      </span>
    </span>
  );
}
