import { useState } from "react"
import { Activity, Blocks, ShieldCheck } from "lucide-react"

import { DesignSystemPage } from "@/app/DesignSystemPage"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { useScaffoldStatus } from "@/hooks/use-scaffold-status"

type Page = "status" | "design-system"

export function App() {
  const [page, setPage] = useState<Page>("status")
  const scaffoldStatus = useScaffoldStatus()

  if (page === "design-system") {
    return <DesignSystemPage onClose={() => setPage("status")} />
  }

  return (
    <main className="min-h-screen bg-background p-6 text-foreground">
      <section aria-labelledby="app-title" className="mx-auto flex max-w-md flex-col gap-5">
        <header className="flex items-center gap-3">
          <div className="rounded-xl bg-primary p-2 text-primary-foreground" aria-hidden="true">
            <Activity className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="text-sm text-muted-foreground">Windows Foundation</p>
            <h1 id="app-title" className="text-xl font-semibold tracking-tight">
              OpenUsage
            </h1>
          </div>
          <Badge variant="secondary">P1</Badge>
        </header>

        <Alert>
          <ShieldCheck aria-hidden="true" />
          <AlertTitle>Secure Platform Boundary</AlertTitle>
          <AlertDescription>
            Provider credentials and raw local data stay in Rust. This WebView receives normalized,
            secret-free status only.
          </AlertDescription>
        </Alert>

        <Card className="gap-4 py-5">
          <CardHeader className="px-5">
            <CardTitle>Runtime Status</CardTitle>
            <CardDescription>Native Windows backend health</CardDescription>
          </CardHeader>
          <CardContent className="px-5">
            <p className="text-sm" role="status" aria-live="polite">
              {scaffoldStatus.message}
            </p>
          </CardContent>
        </Card>

        <Separator />

        <Button variant="outline" onClick={() => setPage("design-system")}>
          <Blocks aria-hidden="true" />
          View Design System
        </Button>

        <p className="text-center text-xs text-muted-foreground">
          Windows 11 x64 · Native provider runtime follows in P2
        </p>
      </section>
    </main>
  )
}
