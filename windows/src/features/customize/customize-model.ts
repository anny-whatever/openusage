import { layoutFor } from "@/features/dashboard/dashboard-model"
import type { MetricLayout, ProviderPresentation, Settings } from "@/platform/app-contract"
import { metricId } from "@/platform/app-contract"

export type Direction = -1 | 1

export function setProviderEnabled(settings: Settings, providerId: string, enabled: boolean): Settings {
  const ids = new Set(settings.enabledProviderIds)
  if (enabled) ids.add(providerId)
  else ids.delete(providerId)
  return { ...settings, enabledProviderIds: [...ids] }
}

export function moveProvider(settings: Settings, providerId: string, direction: Direction): Settings {
  return { ...settings, providerOrder: move(settings.providerOrder, providerId, direction) }
}

export function providerMetricIds(provider: ProviderPresentation): string[] {
  return (provider.snapshot?.lines ?? []).map((line) => metricId(provider.id, line))
}

export function setMetricHidden(
  settings: Settings,
  provider: ProviderPresentation,
  metric: string,
  hidden: boolean,
): Settings {
  const layout = normalizedLayout(settings, provider)
  const hiddenIds = new Set(layout.hiddenMetricIds)
  if (hidden) hiddenIds.add(metric)
  else hiddenIds.delete(metric)
  if (hiddenIds.size >= providerMetricIds(provider).length && providerMetricIds(provider).length > 0) {
    return settings
  }
  const starred = layout.starredMetricIds.filter((id) => !hiddenIds.has(id))
  const onDemand = ensureAlwaysVisible(provider, [...hiddenIds], layout.onDemandMetricIds)
  return saveLayout(settings, provider.id, {
    ...layout,
    hiddenMetricIds: [...hiddenIds],
    starredMetricIds: starred,
    onDemandMetricIds: onDemand,
  })
}

export function setMetricOnDemand(
  settings: Settings,
  provider: ProviderPresentation,
  metric: string,
  onDemand: boolean,
): Settings {
  const layout = normalizedLayout(settings, provider)
  const ids = new Set(layout.onDemandMetricIds)
  if (onDemand) ids.add(metric)
  else ids.delete(metric)
  return saveLayout(settings, provider.id, {
    ...layout,
    onDemandMetricIds: ensureAlwaysVisible(provider, layout.hiddenMetricIds, [...ids]),
  })
}

export function setMetricStarred(
  settings: Settings,
  provider: ProviderPresentation,
  metric: string,
  starred: boolean,
): Settings {
  const layout = normalizedLayout(settings, provider)
  if (layout.hiddenMetricIds.includes(metric)) return settings
  const ids = new Set(layout.starredMetricIds)
  if (starred) ids.add(metric)
  else ids.delete(metric)
  return saveLayout(settings, provider.id, { ...layout, starredMetricIds: [...ids] })
}

export function moveMetric(
  settings: Settings,
  provider: ProviderPresentation,
  metric: string,
  direction: Direction,
): Settings {
  const layout = normalizedLayout(settings, provider)
  return saveLayout(settings, provider.id, {
    ...layout,
    orderedMetricIds: move(layout.orderedMetricIds, metric, direction),
  })
}

export function resetProvider(settings: Settings, providerId: string): Settings {
  const metricLayouts = { ...settings.metricLayouts }
  delete metricLayouts[providerId]
  return { ...settings, metricLayouts }
}

export function resetAllLayout(settings: Settings): Settings {
  return {
    ...settings,
    enabledProviderIds: ["claude", "codex", "cursor"],
    providerOrder: ["claude", "codex", "cursor", "antigravity", "copilot", "devin", "grok", "opencode", "openrouter", "zai"],
    metricLayouts: {},
  }
}

function normalizedLayout(settings: Settings, provider: ProviderPresentation): MetricLayout {
  const layout = layoutFor(settings, provider.id)
  const known = providerMetricIds(provider)
  const knownSet = new Set(known)
  const ordered = [...layout.orderedMetricIds.filter((id) => knownSet.has(id)), ...known.filter((id) => !layout.orderedMetricIds.includes(id))]
  return { ...layout, orderedMetricIds: ordered }
}

function ensureAlwaysVisible(
  provider: ProviderPresentation,
  hiddenIds: string[],
  requestedOnDemandIds: string[],
): string[] {
  const hidden = new Set(hiddenIds)
  const onDemand = new Set(requestedOnDemandIds)
  const enabled = providerMetricIds(provider).filter((id) => !hidden.has(id))
  if (enabled.length > 0 && enabled.every((id) => onDemand.has(id))) onDemand.delete(enabled[0])
  return [...onDemand]
}

function saveLayout(settings: Settings, providerId: string, layout: MetricLayout): Settings {
  return { ...settings, metricLayouts: { ...settings.metricLayouts, [providerId]: layout } }
}

function move(values: string[], value: string, direction: Direction): string[] {
  const next = [...values]
  const index = next.indexOf(value)
  const target = index + direction
  if (index < 0 || target < 0 || target >= next.length) return next
  const sourceValue = next[index]
  const targetValue = next[target]
  if (sourceValue === undefined || targetValue === undefined) return next
  next[index] = targetValue
  next[target] = sourceValue
  return next
}
