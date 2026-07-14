import { useSyncExternalStore } from "react"

const CLOCK_INTERVAL_MS = 30_000
const MAX_SUBSCRIBERS = 2_048

let currentTime = Date.now()
let interval: number | undefined
const subscribers = new Set<() => void>()

function tick() {
  currentTime = Date.now()
  for (const subscriber of subscribers) subscriber()
}
function subscribe(subscriber: () => void) {
  if (subscribers.size >= MAX_SUBSCRIBERS) return () => undefined
  subscribers.add(subscriber)
  if (subscribers.size === 1) {
    tick()
    interval = window.setInterval(tick, CLOCK_INTERVAL_MS)
  }
  return () => {
    subscribers.delete(subscriber)
    if (subscribers.size === 0 && interval !== undefined) {
      window.clearInterval(interval)
      interval = undefined
    }
  }
}

function getSnapshot() {
  return currentTime
}

export function useLiveClock() {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}
