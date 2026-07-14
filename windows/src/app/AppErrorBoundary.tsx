import { Component, type ReactNode } from "react"
import { CircleAlert } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { reportUiError } from "@/platform/app-client"

type State = { failed: boolean }

export class AppErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false }

  static getDerivedStateFromError(): State {
    return { failed: true }
  }

  componentDidCatch(error: Error) {
    void reportUiError(error.name)
  }

  render() {
    if (!this.state.failed) return this.props.children
    return (
      <main className="grid min-h-screen place-items-center bg-background p-6 text-foreground">
        <Alert variant="destructive" className="max-w-sm">
          <CircleAlert aria-hidden="true" />
          <AlertTitle>OpenUsage Could Not Render</AlertTitle>
          <AlertDescription className="space-y-3">
            <p>Your credentials remain in the native Windows service.</p>
            <Button size="sm" variant="outline" onClick={() => window.location.reload()}>
              Reload Interface
            </Button>
          </AlertDescription>
        </Alert>
      </main>
    )
  }
}
