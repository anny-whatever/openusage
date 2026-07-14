import { type ReactNode, useEffect, useRef } from "react"
import { ChartNoAxesCombined, Settings, SlidersHorizontal } from "lucide-react"

import { Button } from "@/components/ui/button"

export type AppPage = "dashboard" | "customize" | "settings" | "design-system"

type AppShellProps = {
  children: ReactNode
  page: AppPage
  saving: boolean
  onNavigate: (page: AppPage) => void
}

const navigation = [
  { id: "dashboard", label: "Dashboard", icon: ChartNoAxesCombined },
  { id: "customize", label: "Customize", icon: SlidersHorizontal },
  { id: "settings", label: "Settings", icon: Settings },
] as const

export function AppShell({ children, page, saving, onNavigate }: AppShellProps) {
  const contentRef = useRef<HTMLDivElement>(null)
  useEffect(() => contentRef.current?.focus(), [page])
  return (
    <main className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      <a
        className="sr-only z-50 rounded-md bg-background p-2 focus:not-sr-only focus:absolute focus:m-2"
        href="#main-content"
      >
        Skip to content
      </a>
      <header className="flex shrink-0 items-center border-b bg-card px-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold">OpenUsage</p>
          <p className="text-xs text-muted-foreground" aria-live="polite">
            {saving ? "Saving changes…" : "Windows usage dashboard"}
          </p>
        </div>
        <nav aria-label="Primary" className="flex items-center gap-1">
          {navigation.map(({ id, label, icon: Icon }) => (
            <Button
              key={id}
              aria-current={page === id ? "page" : undefined}
              aria-label={label}
              size="icon-sm"
              variant={page === id ? "secondary" : "ghost"}
              onClick={() => onNavigate(id)}
            >
              <Icon aria-hidden="true" />
            </Button>
          ))}
        </nav>
      </header>
      <div ref={contentRef} id="main-content" className="min-h-0 flex-1 overflow-y-auto" tabIndex={-1}>
        {children}
      </div>
    </main>
  )
}
