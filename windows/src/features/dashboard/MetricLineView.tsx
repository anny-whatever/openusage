import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { formatMetricValue } from "@/features/dashboard/dashboard-model"
import { useLiveClock } from "@/features/dashboard/live-clock"
import { cn } from "@/lib/utils"
import type { MetricLine } from "@/platform/app-contract"

export function MetricLineView({ line, alwaysShowPacing, meterStyle, resetDisplay }: { line: MetricLine; alwaysShowPacing: boolean; meterStyle: "used" | "remaining"; resetDisplay: "automatic" | "countdown" | "time" }) {
  return (
    <div className="space-y-2 py-2 text-sm">
      <div className="flex items-start justify-between gap-3">
        <span className="font-medium">{line.label}</span>
        <MetricValueView line={line} meterStyle={meterStyle} />
      </div>
      {line.type === "progress" && <ProgressDetails line={line} alwaysShowPacing={alwaysShowPacing} resetDisplay={resetDisplay} />}
      {line.type === "chart" && <MetricChart points={line.points} note={line.note} />}
    </div>
  )
}

function MetricValueView({ line, meterStyle }: { line: MetricLine; meterStyle: "used" | "remaining" }) {
  if (line.type === "text") {
    return <span className="text-right">{line.value}</span>
  }
  if (line.type === "badge") {
    return <Badge variant="secondary">{line.text}</Badge>
  }
  if (line.type === "values") {
    return (
      <span className="flex max-w-[65%] flex-wrap justify-end gap-x-2 gap-y-1 text-right">
        {line.values.map((value, index) => (
          <span key={`${value.label ?? value.kind}-${index}`}>
            {value.label && <span className="text-muted-foreground">{value.label} </span>}
            {value.estimated && <span aria-label="estimated">~</span>}
            {formatMetricValue(value.number, value.kind)}
          </span>
        ))}
      </span>
    )
  }
  if (line.type === "progress") {
    return <span>{formatProgress(line.used, line.limit, line.format, meterStyle)}</span>
  }
  return line.points.length > 0 ? <span>{line.points.at(-1)?.valueLabel ?? line.points.at(-1)?.value}</span> : null
}

function ProgressDetails({
  line,
  alwaysShowPacing,
  resetDisplay,
}: {
  line: Extract<MetricLine, { type: "progress" }>
  alwaysShowPacing: boolean
  resetDisplay: "automatic" | "countdown" | "time"
}) {
  return <LiveProgressDetails alwaysShowPacing={alwaysShowPacing} line={line} resetDisplay={resetDisplay} />
}

function LiveProgressDetails({ line, alwaysShowPacing, resetDisplay }: { line: Extract<MetricLine, { type: "progress" }>; alwaysShowPacing: boolean; resetDisplay: "automatic" | "countdown" | "time" }) {
  const now = useLiveClock()
  const percent = Math.min(100, Math.max(0, (line.used / line.limit) * 100))
  const pace = pacing(line, now)
  return <div className="space-y-1.5"><Progress aria-label={`${line.label}: ${Math.round(percent)} percent used`} className={cn(pace.tone === "critical" && "[&_[data-slot=progress-indicator]]:bg-destructive", pace.tone === "warning" && "[&_[data-slot=progress-indicator]]:bg-warning")} value={percent} /><div className="flex justify-between gap-3 text-xs text-muted-foreground"><span>{resetLabel(line.resetsAt, resetDisplay, now)}</span>{(alwaysShowPacing || pace.tone !== "healthy") && <span>{pace.label}</span>}</div></div>
}

function resetLabel(timestamp: string | undefined, display: "automatic" | "countdown" | "time", now: number) {
  if (!timestamp) return "No reset time"
  const remainingSeconds = Math.max(0, Math.floor((Date.parse(timestamp) - now) / 1_000))
  const hours = Math.floor(remainingSeconds / 3_600)
  const minutes = Math.floor((remainingSeconds % 3_600) / 60)
  if (display === "time" || (display === "automatic" && hours >= 24)) {
    return `Resets ${new Date(timestamp).toLocaleString(undefined, { dateStyle: hours >= 24 ? "short" : undefined, timeStyle: "short" })}`
  }
  return `Resets in ${hours}h ${minutes}m`
}

function MetricChart({ points, note }: { points: { value: number; label: string }[]; note?: string }) {
  const maximum = Math.max(1, ...points.map((point) => point.value))
  return (
    <div>
      <div className="flex h-12 items-end gap-0.5" aria-label={`${points.length} usage history points`}>
        {points.map((point) => (
          <span
            key={point.label}
            aria-label={`${point.label}: ${point.value}`}
            className="min-h-px flex-1 rounded-t-sm bg-chart-1"
            style={{ height: `${Math.max(2, (point.value / maximum) * 100)}%` }}
          />
        ))}
      </div>
      {note && <p className="mt-1 text-xs text-muted-foreground">{note}</p>}
    </div>
  )
}

function pacing(line: Extract<MetricLine, { type: "progress" }>, now: number) {
  const consumed = line.used / line.limit
  if (!line.resetsAt || !line.periodDurationMs) {
    if (consumed >= 0.9) return { tone: "critical", label: "Limit nearly reached" }
    if (consumed >= 0.8) return { tone: "warning", label: "Usage is high" }
    return { tone: "healthy", label: "On pace" }
  }
  const remaining = Date.parse(line.resetsAt) - now
  const elapsed = Math.max(0, Math.min(1, 1 - remaining / line.periodDurationMs))
  if (consumed >= 0.9 || consumed - elapsed >= 0.2) return { tone: "critical", label: "Running out early" }
  if (consumed - elapsed >= 0.08) return { tone: "warning", label: "Usage is ahead of pace" }
  return { tone: "healthy", label: "On pace" }
}

function formatProgress(
  used: number,
  limit: number,
  format: "percent" | "dollars" | { kind: "count"; suffix: string },
  meterStyle: "used" | "remaining",
) {
  const shown = meterStyle === "remaining" ? Math.max(0, limit - used) : used
  const suffix = meterStyle === "remaining" ? "left" : "used"
  if (format === "percent") return `${Math.round((shown / limit) * 100)}% ${suffix}`
  if (format === "dollars") return `${formatMetricValue(shown, "dollars")} ${suffix}`
  return `${formatMetricValue(shown, "count")} ${format.suffix} ${suffix}`
}
