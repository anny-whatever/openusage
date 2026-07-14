import { useEffect, useState } from "react"

import { readScaffoldStatus } from "@/platform/scaffold-client"

type StatusState = {
  message: string
}

const initialStatus: StatusState = { message: "Connecting to Windows backend…" }

export function useScaffoldStatus(): StatusState {
  const [status, setStatus] = useState(initialStatus)

  useEffect(() => {
    const abortController = new AbortController()

    async function loadStatus() {
      try {
        const value = await readScaffoldStatus(abortController.signal)
        if (!value.trustedBackend) {
          throw new Error("Backend trust assertion failed")
        }
        setStatus({ message: `Windows ${value.architecture} backend connected` })
      } catch {
        if (!abortController.signal.aborted) {
          setStatus({ message: "Windows backend unavailable" })
        }
      }
    }

    void loadStatus()
    return () => abortController.abort()
  }, [])

  return status
}

