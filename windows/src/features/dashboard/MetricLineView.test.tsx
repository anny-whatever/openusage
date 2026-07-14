import { render } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { MetricLineView } from "@/features/dashboard/MetricLineView"

const line = {
  type: "progress" as const,
  label: "Weekly",
  used: 40,
  limit: 100,
  format: "percent" as const,
  resetsAt: "2026-07-20T00:00:00Z",
  periodDurationMs: 7 * 24 * 60 * 60 * 1_000,
}

describe("MetricLineView live clock", () => {
  it("shares one bounded interval across progress rows and releases it after unmount", () => {
    const setIntervalSpy = vi.spyOn(window, "setInterval")
    const clearIntervalSpy = vi.spyOn(window, "clearInterval")
    const view = render(
      <>
        <MetricLineView line={line} alwaysShowPacing={false} meterStyle="remaining" resetDisplay="countdown" />
        <MetricLineView line={{ ...line, label: "Session" }} alwaysShowPacing={false} meterStyle="remaining" resetDisplay="countdown" />
      </>,
    )

    expect(setIntervalSpy).toHaveBeenCalledTimes(1)
    view.unmount()
    expect(clearIntervalSpy).toHaveBeenCalledTimes(1)
    setIntervalSpy.mockRestore()
    clearIntervalSpy.mockRestore()
  })
})
