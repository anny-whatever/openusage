import { describe, expect, it } from "vitest"

import { moveProvider, setMetricHidden, setMetricOnDemand, setMetricStarred } from "@/features/customize/customize-model"
import { metricId } from "@/platform/app-contract"
import { providerFixture, settingsFixture } from "@/test-fixtures"

function providerWithMetrics() {
  const provider = providerFixture("claude")
  provider.snapshot = {
    providerId: "claude",
    displayName: "Claude",
    refreshedAt: "2026-07-14T00:00:00Z",
    lines: [
      { type: "text", label: "Account", value: "Personal" },
      { type: "text", label: "Plan", value: "Pro" },
    ],
  }
  return provider
}

describe("customization model", () => {
  it("never hides every metric or leaves every enabled metric on demand", () => {
    const provider = providerWithMetrics()
    const [account, plan] = provider.snapshot!.lines.map((line) => metricId(provider.id, line))
    let settings = settingsFixture()
    settings = setMetricHidden(settings, provider, account, true)
    settings = setMetricHidden(settings, provider, plan, true)
    expect(settings.metricLayouts.claude.hiddenMetricIds).toEqual([account])

    settings = setMetricOnDemand(settings, provider, plan, true)
    expect(settings.metricLayouts.claude.onDemandMetricIds).not.toContain(plan)
  })

  it("removes stars when a metric is hidden and keeps provider order bounded", () => {
    const provider = providerWithMetrics()
    const metric = metricId(provider.id, provider.snapshot!.lines[0])
    let settings = setMetricStarred(settingsFixture(), provider, metric, true)
    settings = setMetricHidden(settings, provider, metric, true)
    expect(settings.metricLayouts.claude.starredMetricIds).not.toContain(metric)
    expect(moveProvider(settings, "claude", -1).providerOrder[0]).toBe("claude")
  })
})
