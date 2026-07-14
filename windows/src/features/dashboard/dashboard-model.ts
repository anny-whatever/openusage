import type {
  MetricLayout,
  MetricLine,
  ProviderPresentation,
  Settings,
} from "@/platform/app-contract"
import { metricId } from "@/platform/app-contract"

export function enabledProviders(
  providers: ProviderPresentation[],
  settings: Settings,
): ProviderPresentation[] {
  const enabled = new Set(settings.enabledProviderIds)
  const byId = new Map(providers.map((provider) => [provider.id, provider]))
  return settings.providerOrder
    .filter((providerId) => enabled.has(providerId))
    .map((providerId) => byId.get(providerId))
    .filter((provider): provider is ProviderPresentation => Boolean(provider))
}

export function layoutFor(settings: Settings, providerId: string): MetricLayout {
  return (
    settings.metricLayouts[providerId] ?? {
      orderedMetricIds: [],
      hiddenMetricIds: [],
      onDemandMetricIds: [],
      starredMetricIds: [],
    }
  )
}

export function orderedLines(
  provider: ProviderPresentation,
  settings: Settings,
): { id: string; line: MetricLine }[] {
  const lines = provider.snapshot?.lines ?? []
  const layout = layoutFor(settings, provider.id)
  const order = new Map(layout.orderedMetricIds.map((id, index) => [id, index]))
  return lines
    .map((line, sourceIndex) => ({ id: metricId(provider.id, line), line, sourceIndex }))
    .filter(({ id }) => !layout.hiddenMetricIds.includes(id))
    .sort((left, right) => {
      const leftOrder = order.get(left.id) ?? Number.MAX_SAFE_INTEGER
      const rightOrder = order.get(right.id) ?? Number.MAX_SAFE_INTEGER
      return leftOrder - rightOrder || left.sourceIndex - right.sourceIndex
    })
    .map(({ id, line }) => ({ id, line }))
}

export function totalSpend(providers: ProviderPresentation[], settings: Settings): number | undefined {
  const spendProviders = new Set(["claude", "codex", "cursor", "grok", "opencode"])
  let total = 0
  let found = false
  for (const provider of enabledProviders(providers, settings)) {
    if (!spendProviders.has(provider.id)) continue
    for (const line of provider.snapshot?.lines ?? []) {
      if (line.type !== "values" || line.label !== "Today") continue
      for (const value of line.values) {
        if (value.kind !== "dollars") continue
        total += value.number
        found = true
      }
    }
  }
  return found ? total : undefined
}

export function isStale(refreshedAt: string, now = Date.now()): boolean {
  const timestamp = Date.parse(refreshedAt)
  return !Number.isFinite(timestamp) || now - timestamp >= 5 * 60 * 1000
}

export function formatMetricValue(number: number, kind: "percent" | "dollars" | "count") {
  if (kind === "dollars") {
    return new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" }).format(number)
  }
  if (kind === "percent") return `${Math.round(number)}%`
  return new Intl.NumberFormat().format(number)
}
