import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "@/app/App"
import { readScaffoldStatus } from "@/platform/scaffold-client"

vi.mock("@/platform/scaffold-client", () => ({
  readScaffoldStatus: vi.fn(),
}))

const readScaffoldStatusMock = vi.mocked(readScaffoldStatus)

describe("App", () => {
  beforeEach(() => {
    readScaffoldStatusMock.mockResolvedValue({
      platform: "windows",
      architecture: "x86_64",
      trustedBackend: true,
    })
  })

  it("shows the trusted Windows runtime status", async () => {
    render(<App />)

    expect(screen.getByRole("heading", { name: "OpenUsage" })).toBeInTheDocument()
    expect(await screen.findByText("Windows x86_64 backend connected")).toBeInTheDocument()
  })

  it("surfaces a backend connection failure without inventing success", async () => {
    readScaffoldStatusMock.mockRejectedValue(new Error("IPC unavailable"))

    render(<App />)

    expect(await screen.findByText("Windows backend unavailable")).toBeInTheDocument()
  })

  it("opens and closes the keyboard-accessible design system page", async () => {
    const user = userEvent.setup()
    render(<App />)

    await user.click(screen.getByRole("button", { name: "View Design System" }))
    expect(screen.getByRole("heading", { name: "Design System" })).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Back to runtime status" }))
    expect(screen.getByRole("heading", { name: "OpenUsage" })).toBeInTheDocument()
  })
})
