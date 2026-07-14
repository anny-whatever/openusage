import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "@/app/App"
import { readAppBootstrap, writeSettings } from "@/platform/app-client"
import { bootstrapFixture } from "@/test-fixtures"

vi.mock("@/platform/app-client", () => ({
  readAppBootstrap: vi.fn(),
  writeSettings: vi.fn(),
  writeApiKey: vi.fn(),
  removeApiKey: vi.fn(),
}))

const readAppBootstrapMock = vi.mocked(readAppBootstrap)
const writeSettingsMock = vi.mocked(writeSettings)

describe("App", () => {
  beforeEach(() => {
    const bootstrap = bootstrapFixture()
    readAppBootstrapMock.mockResolvedValue(bootstrap)
    writeSettingsMock.mockResolvedValue(bootstrap)
  })

  it("renders unauthorized providers without inventing usage", async () => {
    render(<App />)

    expect(await screen.findByRole("heading", { name: "Dashboard" })).toBeInTheDocument()
    expect(screen.getAllByText("Sign in with the provider's Windows app or CLI, then refresh.")).toHaveLength(3)
    expect(screen.queryByText("$0.00")).not.toBeInTheDocument()
  })

  it("surfaces a backend connection failure and supports retry", async () => {
    readAppBootstrapMock.mockRejectedValue(new Error("IPC unavailable"))
    render(<App />)

    expect(await screen.findByText("Windows Service Unavailable")).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Try Again" })).toBeEnabled()
  })

  it("navigates dashboard, customize, settings, and design system by accessible controls", async () => {
    const user = userEvent.setup()
    render(<App />)
    await screen.findByRole("heading", { name: "Dashboard" })

    await user.click(screen.getByRole("button", { name: "Customize" }))
    expect(screen.getByRole("heading", { name: "Customize" })).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Settings" }))
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Open design system" }))
    expect(screen.getByRole("heading", { name: "Design System" })).toBeInTheDocument()
  })

  it("persists a keyboard-operated preference through the typed settings command", async () => {
    const user = userEvent.setup()
    render(<App />)
    await screen.findByRole("heading", { name: "Dashboard" })

    const settingsButton = screen.getByRole("button", { name: "Settings" })
    settingsButton.focus()
    await user.keyboard("{Enter}")
    const totalSpendSwitch = screen.getByRole("switch", { name: "Show Total Spend" })
    totalSpendSwitch.focus()
    await user.keyboard(" ")

    expect(writeSettingsMock).toHaveBeenCalledWith(
      expect.objectContaining({ showTotalSpend: false }),
    )
  })
})
