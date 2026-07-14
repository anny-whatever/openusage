import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it } from "vitest"

import { DashboardPage } from "@/features/dashboard/DashboardPage"
import { bootstrapFixture, providerFixture, settingsFixture } from "@/test-fixtures"

describe("DashboardPage", () => {
  it("renders every normalized metric kind plus partial and stale states", () => {
    const provider = providerFixture("claude")
    provider.snapshot = {
      providerId: "claude",
      displayName: "Claude",
      refreshedAt: "2026-01-01T00:00:00Z",
      warning: "One endpoint is unavailable.",
      lines: [
        { type: "text", label: "Account", value: "Personal" },
        { type: "badge", label: "Plan", text: "Pro" },
        { type: "values", label: "Today", values: [{ number: 12.5, kind: "dollars", estimated: false }] },
        { type: "progress", label: "Weekly", used: 35, limit: 100, format: "percent" },
        { type: "chart", label: "History", points: [{ label: "Mon", value: 2 }, { label: "Tue", value: 4 }] },
      ],
    }
    const bootstrap = bootstrapFixture([provider], settingsFixture({ enabledProviderIds: ["claude"], providerOrder: ["claude"] }))

    render(<DashboardPage bootstrap={bootstrap} loading={false} onCustomize={() => undefined} onDismissHint={() => undefined} onReload={() => undefined} />)

    expect(screen.getByText("Stale")).toBeInTheDocument()
    expect(screen.getByText("Partial Data")).toBeInTheDocument()
    expect(screen.getByText("Personal")).toBeInTheDocument()
    expect(screen.getAllByText("Pro").length).toBeGreaterThan(0)
    expect(screen.getAllByText("$12.50")).toHaveLength(2)
    expect(screen.getByRole("progressbar", { name: "Weekly: 35 percent used" })).toBeInTheDocument()
    expect(screen.getByText("65% left")).toBeInTheDocument()
    expect(screen.getByLabelText("2 usage history points")).toBeInTheDocument()
  })

  it("renders the empty state when every provider is disabled", () => {
    const bootstrap = bootstrapFixture([], settingsFixture({ enabledProviderIds: [], providerOrder: [] }))
    render(<DashboardPage bootstrap={bootstrap} loading={false} onCustomize={() => undefined} onDismissHint={() => undefined} onReload={() => undefined} />)
    expect(screen.getByText("No Providers Enabled")).toBeInTheDocument()
  })

  it("shows a friendly provider error while preserving its safe snapshot", () => {
    const provider = providerFixture("claude")
    provider.snapshot = {
      providerId: "claude",
      displayName: "Claude",
      refreshedAt: "2026-07-14T00:00:00Z",
      errorCategory: "network",
      lines: [{ type: "text", label: "Account", value: "Personal" }],
    }
    const bootstrap = bootstrapFixture(
      [provider],
      settingsFixture({ enabledProviderIds: ["claude"], providerOrder: ["claude"] }),
    )

    render(
      <DashboardPage
        bootstrap={bootstrap}
        loading={false}
        onCustomize={() => undefined}
        onDismissHint={() => undefined}
        onReload={() => undefined}
      />,
    )

    expect(screen.getByText("Provider Unavailable")).toBeInTheDocument()
    expect(screen.getByText("Personal")).toBeInTheDocument()
  })

  it("opens secret-free share and confirmed reset flows without enabling unfinished native actions", async () => {
    const user = userEvent.setup()
    const provider = providerFixture("codex")
    provider.snapshot = {
      providerId: "codex",
      displayName: "Codex",
      refreshedAt: "2026-07-14T00:00:00Z",
      lines: [
        {
          type: "values",
          label: "Rate Limit Resets",
          values: [{ number: 1, kind: "count", label: "available", estimated: false }],
          expiriesAt: ["2026-07-20T00:00:00Z"],
        },
      ],
    }
    const bootstrap = bootstrapFixture(
      [provider],
      settingsFixture({ enabledProviderIds: ["codex"], providerOrder: ["codex"] }),
    )
    render(
      <DashboardPage
        bootstrap={bootstrap}
        loading={false}
        onCustomize={() => undefined}
        onDismissHint={() => undefined}
        onReload={() => undefined}
      />,
    )

    await user.click(screen.getByRole("button", { name: "Share Codex" }))
    expect(screen.getByText("normalized usage only", { exact: false })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Copy Share Image" })).toBeDisabled()
    await user.click(screen.getByRole("button", { name: "Close" }))
    await user.click(screen.getByRole("button", { name: "Show Links" }))
    await user.click(screen.getByRole("button", { name: "Claim Reset" }))
    expect(screen.getByRole("heading", { name: "Claim a Codex Reset?" })).toBeInTheDocument()
    expect(screen.getAllByRole("button", { name: "Claim Reset" }).at(-1)).toBeDisabled()
  })
})
