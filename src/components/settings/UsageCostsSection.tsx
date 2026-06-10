import { useMemo, useState } from "react";
import type { ApiUsageEvent, ApiUsageService } from "../../types";
import {
  SettingsSectionPanel,
  settingsButtonSecondaryClass,
} from "./SettingsControls";

type RangeDays = 7 | 30 | 90;

type DailyUsage = {
  key: string;
  label: string;
  cost: number;
  count: number;
  unpriced: number;
};

type ServiceUsage = {
  service: ApiUsageService;
  label: string;
  cost: number;
  count: number;
  unpriced: number;
  colorClass: string;
};

const SERVICE_META: Record<
  ApiUsageService,
  { label: string; colorClass: string; pillClass: string }
> = {
  geminiTranslation: {
    label: "Gemini 翻訳",
    colorClass: "bg-blue-500 dark:bg-blue-400",
    pillClass: "bg-blue-500/10 text-blue-700 dark:text-blue-300",
  },
  geminiFinalization: {
    label: "Gemini 整形",
    colorClass: "bg-emerald-500 dark:bg-emerald-400",
    pillClass: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  },
  geminiAudioInput: {
    label: "Gemini 音声入力",
    colorClass: "bg-violet-500 dark:bg-violet-400",
    pillClass: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
  },
  openAiTranscription: {
    label: "OpenAI 文字起こし",
    colorClass: "bg-rose-500 dark:bg-rose-400",
    pillClass: "bg-rose-500/10 text-rose-700 dark:text-rose-300",
  },
  googleSpeechToText: {
    label: "Google Speech-to-Text",
    colorClass: "bg-amber-500 dark:bg-amber-400",
    pillClass: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
};

const RANGE_OPTIONS: RangeDays[] = [7, 30, 90];

export function UsageCostsSection({
  events,
  refreshing,
  onRefresh,
}: {
  events: ApiUsageEvent[];
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const [rangeDays, setRangeDays] = useState<RangeDays>(30);
  const report = useMemo(() => buildUsageReport(events, rangeDays), [events, rangeDays]);
  const maxDailyCost = Math.max(...report.daily.map((day) => day.cost), 0);
  const maxServiceCost = Math.max(...report.services.map((service) => service.cost), 0);
  const chartMinWidth = Math.max(rangeDays * 24, 520);

  return (
    <SettingsSectionPanel
      title="利用料金（概算）"
      description="この端末で実行したAPI呼び出しを直近90日分だけ保存し、公式単価ベースで概算します。実際の請求額は各プロバイダの請求画面で確認してください。"
    >
      <div className="flex flex-wrap items-center justify-between gap-3 sm:col-span-2">
        <div className="flex rounded-lg border border-edge bg-sunken p-0.5">
          {RANGE_OPTIONS.map((days) => (
            <button
              key={days}
              type="button"
              onClick={() => setRangeDays(days)}
              className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors duration-150 focus-ring ${
                rangeDays === days
                  ? "bg-surface text-ink shadow-sm"
                  : "text-ink-mid hover:text-ink"
              }`}
            >
              {days}日
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          className={settingsButtonSecondaryClass}
        >
          {refreshing ? "更新中…" : "更新"}
        </button>
      </div>

      <UsageSummaryCard label="合計" value={formatUsd(report.totalCost)} />
      <UsageSummaryCard label="API呼び出し" value={`${report.totalCount.toLocaleString()}回`} />
      <UsageSummaryCard
        label="未算出"
        value={`${report.unpricedCount.toLocaleString()}件`}
        tone={report.unpricedCount > 0 ? "warning" : "neutral"}
      />
      <UsageSummaryCard label="保存期間" value="90日" />

      <section className="rounded-xl border border-edge bg-sunken p-4 sm:col-span-2">
        <div className="mb-3 flex items-end justify-between gap-3">
          <h3 className="text-sm font-semibold text-ink">日別</h3>
          <span className="text-xs text-ink-mid">
            最大 {formatUsd(maxDailyCost)}
          </span>
        </div>
        <div className="overflow-x-auto pb-1">
          <div
            className="flex h-44 items-end gap-1.5"
            style={{ minWidth: `${chartMinWidth}px` }}
          >
            {report.daily.map((day) => {
              const height = maxDailyCost > 0 ? Math.max((day.cost / maxDailyCost) * 100, 3) : 0;
              return (
                <div key={day.key} className="flex h-full min-w-0 flex-1 flex-col items-center gap-2">
                  <div className="flex h-32 w-full items-end rounded-md bg-surface px-1">
                    <div
                      className="w-full rounded-t bg-accent"
                      style={{ height: `${height}%` }}
                      title={`${day.key}: ${formatUsd(day.cost)} / ${day.count}回`}
                    />
                  </div>
                  <span className="w-full truncate text-center text-[10px] text-ink-faint">
                    {day.label}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </section>

      <section className="rounded-xl border border-edge bg-sunken p-4 sm:col-span-2">
        <div className="mb-3 flex items-end justify-between gap-3">
          <h3 className="text-sm font-semibold text-ink">用途別</h3>
          <span className="text-xs text-ink-mid">
            {report.services.length.toLocaleString()}用途
          </span>
        </div>
        {report.services.length ? (
          <div className="space-y-3">
            {report.services.map((service) => {
              const width = maxServiceCost > 0 ? Math.max((service.cost / maxServiceCost) * 100, 2) : 0;
              return (
                <div key={service.service} className="grid gap-1.5">
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-sm font-medium text-ink">
                      {service.label}
                    </span>
                    <span className="shrink-0 text-xs tabular-nums text-ink-mid">
                      {formatUsd(service.cost)} / {service.count.toLocaleString()}回
                    </span>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-surface">
                    <div
                      className={`h-full rounded-full ${service.colorClass}`}
                      style={{ width: `${width}%` }}
                    />
                  </div>
                  {service.unpriced > 0 ? (
                    <p className="text-[11px] text-warn">
                      未算出 {service.unpriced.toLocaleString()}件
                    </p>
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : (
          <EmptyUsageState />
        )}
      </section>

      <section className="sm:col-span-2">
        <div className="mb-3 flex items-end justify-between gap-3">
          <h3 className="text-sm font-semibold text-ink">記録一覧</h3>
          <span className="text-xs text-ink-mid">最新20件</span>
        </div>
        {report.recent.length ? (
          <div className="overflow-x-auto rounded-xl border border-edge bg-sunken">
            <table className="min-w-full text-left text-xs">
              <thead className="border-b border-edge text-ink-mid">
                <tr>
                  <th className="whitespace-nowrap px-3 py-2 font-medium">日時</th>
                  <th className="whitespace-nowrap px-3 py-2 font-medium">用途</th>
                  <th className="whitespace-nowrap px-3 py-2 font-medium">モデル</th>
                  <th className="whitespace-nowrap px-3 py-2 font-medium">利用量</th>
                  <th className="whitespace-nowrap px-3 py-2 text-right font-medium">概算</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-edge">
                {report.recent.map((event) => {
                  const meta = SERVICE_META[event.service];
                  return (
                    <tr key={event.id}>
                      <td className="whitespace-nowrap px-3 py-2 text-ink-mid">
                        {formatDateTime(event.timestampMs)}
                      </td>
                      <td className="px-3 py-2">
                        <span
                          className={`inline-flex max-w-40 items-center truncate rounded-full px-2 py-0.5 ${meta.pillClass}`}
                          title={meta.label}
                        >
                          {meta.label}
                        </span>
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 font-mono text-[11px] text-ink-mid">
                        {event.model}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-ink-mid">
                        {formatUsageQuantity(event)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums text-ink">
                        {formatUsd(event.estimatedCostUsd)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <EmptyUsageState />
        )}
      </section>
    </SettingsSectionPanel>
  );
}

function UsageSummaryCard({
  label,
  value,
  tone = "neutral",
}: {
  label: string;
  value: string;
  tone?: "neutral" | "warning";
}) {
  return (
    <div className="rounded-xl border border-edge bg-sunken px-4 py-3">
      <p className="text-xs font-medium text-ink-mid">{label}</p>
      <p
        className={`mt-1 text-lg font-semibold tabular-nums ${
          tone === "warning" ? "text-warn" : "text-ink"
        }`}
      >
        {value}
      </p>
    </div>
  );
}

function EmptyUsageState() {
  return (
    <div className="rounded-xl border border-edge bg-sunken px-4 py-6 text-center text-sm text-ink-mid">
      まだ利用記録がありません。
    </div>
  );
}

function buildUsageReport(events: ApiUsageEvent[], rangeDays: RangeDays) {
  const cutoff = startOfLocalDay(Date.now() - (rangeDays - 1) * 24 * 60 * 60 * 1000);
  const filtered = events.filter((event) => event.timestampMs >= cutoff);
  const daily = buildDailyUsage(filtered, rangeDays);
  const services = buildServiceUsage(filtered);
  const totalCost = sumCost(filtered);
  const totalCount = filtered.reduce((sum, event) => sum + event.requestCount, 0);
  const unpricedCount = filtered.filter((event) => event.estimatedCostUsd === null).length;
  const recent = [...filtered]
    .sort((a, b) => b.timestampMs - a.timestampMs)
    .slice(0, 20);

  return {
    daily,
    services,
    totalCost,
    totalCount,
    unpricedCount,
    recent,
  };
}

function buildDailyUsage(events: ApiUsageEvent[], rangeDays: RangeDays): DailyUsage[] {
  const daily = new Map<string, DailyUsage>();
  const start = new Date(startOfLocalDay(Date.now() - (rangeDays - 1) * 24 * 60 * 60 * 1000));

  for (let index = 0; index < rangeDays; index += 1) {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = localDateKey(date.getTime());
    daily.set(key, {
      key,
      label: `${date.getDate()}`,
      cost: 0,
      count: 0,
      unpriced: 0,
    });
  }

  for (const event of events) {
    const key = localDateKey(event.timestampMs);
    const bucket = daily.get(key);
    if (!bucket) continue;
    bucket.cost += event.estimatedCostUsd ?? 0;
    bucket.count += event.requestCount;
    if (event.estimatedCostUsd === null) {
      bucket.unpriced += 1;
    }
  }

  return [...daily.values()];
}

function buildServiceUsage(events: ApiUsageEvent[]): ServiceUsage[] {
  const services = new Map<ApiUsageService, ServiceUsage>();
  for (const event of events) {
    const meta = SERVICE_META[event.service];
    const current =
      services.get(event.service) ??
      {
        service: event.service,
        label: meta.label,
        colorClass: meta.colorClass,
        cost: 0,
        count: 0,
        unpriced: 0,
      };
    current.cost += event.estimatedCostUsd ?? 0;
    current.count += event.requestCount;
    if (event.estimatedCostUsd === null) {
      current.unpriced += 1;
    }
    services.set(event.service, current);
  }
  return [...services.values()].sort((a, b) => b.cost - a.cost || b.count - a.count);
}

function sumCost(events: ApiUsageEvent[]) {
  return events.reduce((sum, event) => sum + (event.estimatedCostUsd ?? 0), 0);
}

function startOfLocalDay(ms: number) {
  const date = new Date(ms);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function localDateKey(ms: number) {
  const date = new Date(ms);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function formatDateTime(ms: number) {
  return new Intl.DateTimeFormat("ja-JP", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

function formatUsd(value: number | null) {
  if (value === null) return "未算出";
  if (value > 0 && value < 0.0001) return "<$0.0001";
  const digits = value < 1 ? 4 : 2;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

function formatUsageQuantity(event: ApiUsageEvent) {
  if (event.durationSecs !== null) {
    return `${formatSeconds(event.durationSecs)}`;
  }
  const parts = [];
  if (event.inputTokens !== null) {
    parts.push(`入力 ${event.inputTokens.toLocaleString()}`);
  }
  if (event.outputTokens !== null) {
    parts.push(`出力 ${event.outputTokens.toLocaleString()}`);
  }
  if (event.audioInputTokens !== null) {
    parts.push(`音声 ${event.audioInputTokens.toLocaleString()}`);
  }
  return parts.length ? parts.join(" / ") : `${event.requestCount.toLocaleString()}回`;
}

function formatSeconds(value: number) {
  if (value >= 60) {
    const minutes = Math.floor(value / 60);
    const seconds = Math.round(value % 60);
    return `${minutes}分${seconds}秒`;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)}秒`;
}
