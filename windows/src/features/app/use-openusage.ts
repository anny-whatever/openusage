import { useCallback, useEffect, useRef, useState } from "react"

import { readAppBootstrap, removeApiKey, writeApiKey, writeSettings } from "@/platform/app-client"
import type { AppBootstrap, Settings } from "@/platform/app-contract"

type AppModel = {
  bootstrap?: AppBootstrap
  error?: string
  loading: boolean
  saving: boolean
  canUndo: boolean
  reload: () => void
  updateSettings: (update: (current: Settings) => Settings) => void
  undo: () => void
  saveApiKey: (providerId: string, key: string) => Promise<void>
  deleteApiKey: (providerId: string) => Promise<void>
}

const MAX_UNDO_ENTRIES = 20

export function useOpenUsage(): AppModel {
  const [bootstrap, setBootstrap] = useState<AppBootstrap>()
  const [error, setError] = useState<string>()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [undoCount, setUndoCount] = useState(0)
  const [reloadGeneration, setReloadGeneration] = useState(0)
  const settingsRef = useRef<Settings | undefined>(undefined)
  const undoRef = useRef<Settings[]>([])
  const writeQueueRef = useRef(Promise.resolve())
  const pendingWritesRef = useRef(0)
  const writeGenerationRef = useRef(0)

  const applyBootstrap = useCallback((next: AppBootstrap) => {
    settingsRef.current = next.settings
    setBootstrap(next)
    setError(undefined)
  }, [])

  useEffect(() => {
    const abortController = new AbortController()
    readAppBootstrap(abortController.signal)
      .then(applyBootstrap)
      .catch(() => {
        if (!abortController.signal.aborted) {
          setError("OpenUsage could not connect to its Windows service.")
        }
      })
      .finally(() => {
        if (!abortController.signal.aborted) setLoading(false)
      })
    return () => abortController.abort()
  }, [applyBootstrap, reloadGeneration])

  const persist = useCallback(
    (next: Settings) => {
      writeGenerationRef.current += 1
      const generation = writeGenerationRef.current
      pendingWritesRef.current += 1
      setSaving(true)
      writeQueueRef.current = writeQueueRef.current
        .then(() => writeSettings(next))
        .then((saved) => {
          if (generation === writeGenerationRef.current) applyBootstrap(saved)
        })
        .catch(() => {
          if (generation === writeGenerationRef.current) {
            setError("Your latest setting could not be saved.")
          }
        })
        .finally(() => {
          pendingWritesRef.current -= 1
          if (pendingWritesRef.current === 0) setSaving(false)
        })
    },
    [applyBootstrap],
  )

  const updateSettings = useCallback(
    (update: (current: Settings) => Settings) => {
      const current = settingsRef.current
      if (!current) return
      const next = update(structuredClone(current))
      undoRef.current = [...undoRef.current.slice(-(MAX_UNDO_ENTRIES - 1)), current]
      setUndoCount(undoRef.current.length)
      settingsRef.current = next
      setBootstrap((value) => (value ? { ...value, settings: next } : value))
      persist(next)
    },
    [persist],
  )

  const undo = useCallback(() => {
    const previous = undoRef.current.pop()
    if (!previous) return
    setUndoCount(undoRef.current.length)
    settingsRef.current = previous
    setBootstrap((value) => (value ? { ...value, settings: previous } : value))
    persist(previous)
  }, [persist])

  const saveApiKey = useCallback(async (providerId: string, key: string) => {
    await writeApiKey(providerId, key)
    setReloadGeneration((value) => value + 1)
  }, [])

  const deleteApiKey = useCallback(async (providerId: string) => {
    await removeApiKey(providerId)
    setReloadGeneration((value) => value + 1)
  }, [])

  return {
    bootstrap,
    error,
    loading,
    saving,
    canUndo: undoCount > 0,
    reload: () => {
      setLoading(true)
      setReloadGeneration((value) => value + 1)
    },
    updateSettings,
    undo,
    saveApiKey,
    deleteApiKey,
  }
}
