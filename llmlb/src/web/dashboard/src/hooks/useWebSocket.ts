import { useEffect, useRef, useCallback, useState } from 'react'
import { queryKeys } from '@/lib/query-keys';
import { useQueryClient } from '@tanstack/react-query'

export type DashboardEventType =
  | 'connected'
  | 'NodeRegistered'
  | 'NodeStatusChanged'
  | 'MetricsUpdated'
  | 'NodeRemoved'
  | 'TpsUpdated'
  | 'UpdateStateChanged'

export interface DashboardEvent {
  type: DashboardEventType
  data?: {
    runtime_id?: string
    endpoint_id?: string
    machine_name?: string
    ip_address?: string
    status?: string
    old_status?: string
    new_status?: string
    cpu_usage?: number
    memory_usage?: number
    gpu_usage?: number
    model_id?: string
    tps?: number
    output_tokens?: number
    duration_ms?: number
  }
  message?: string
}

interface UseWebSocketOptions {
  onMessage?: (event: DashboardEvent) => void
  onConnect?: () => void
  onDisconnect?: () => void
  enabled?: boolean
}

/** Calculate reconnect delay with exponential backoff and jitter */
function getReconnectDelay(attempt: number): number {
  const base = Math.min(1000 * Math.pow(2, attempt), 30000)
  const jitter = base * 0.2 * Math.random()
  return base + jitter
}

export function useWebSocket(options: UseWebSocketOptions = {}) {
  const {
    onMessage,
    onConnect,
    onDisconnect,
    enabled = true,
  } = options

  const queryClient = useQueryClient()
  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const reconnectAttemptRef = useRef(0)
  const connectRef = useRef<() => void>(() => {})
  const shouldReconnectRef = useRef(false)
  const [isConnected, setIsConnected] = useState(false)
  const [lastEvent, setLastEvent] = useState<DashboardEvent | null>(null)

  // Stabilize callback references with useRef to prevent infinite reconnection loops.
  // Without this, inline callbacks passed by callers create new references every render,
  // which would trigger useCallback/useEffect dependency changes and cause
  // connect → disconnect → reconnect cycles on every render.
  const onMessageRef = useRef(onMessage)
  const onConnectRef = useRef(onConnect)
  const onDisconnectRef = useRef(onDisconnect)
  useEffect(() => {
    onMessageRef.current = onMessage
    onConnectRef.current = onConnect
    onDisconnectRef.current = onDisconnect
  })

  const scheduleReconnect = useCallback(() => {
    if (!shouldReconnectRef.current) return

    const delay = getReconnectDelay(reconnectAttemptRef.current)
    reconnectAttemptRef.current += 1
    reconnectTimeoutRef.current = setTimeout(() => connectRef.current(), delay)
  }, [])

  const connect = useCallback(() => {
    shouldReconnectRef.current = true

    // Determine WebSocket URL based on current location
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws/dashboard`

    try {
      const ws = new WebSocket(wsUrl)
      wsRef.current = ws

      ws.onopen = () => {
        reconnectAttemptRef.current = 0
        setIsConnected(true)
        onConnectRef.current?.()
      }

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as DashboardEvent
          setLastEvent(data)
          onMessageRef.current?.(data)

          // Invalidate relevant queries based on event type
          // Query keys must match those used in Dashboard.tsx
          switch (data.type) {
            case 'NodeRegistered':
            case 'NodeRemoved':
            case 'NodeStatusChanged':
              // Invalidate dashboard overview query (includes endpoints, stats)
              queryClient.invalidateQueries({ queryKey: queryKeys.dashboardOverview })
              queryClient.invalidateQueries({ queryKey: queryKeys.requestResponses })
              break
            case 'MetricsUpdated':
              // Invalidate dashboard overview for metrics updates
              queryClient.invalidateQueries({ queryKey: queryKeys.dashboardOverview })
              break
            case 'TpsUpdated':
              // SPEC-4bb5b55f: Invalidate TPS data for the affected endpoint
              if (data.data?.endpoint_id) {
                queryClient.invalidateQueries({ queryKey: queryKeys.endpointModelTps(data.data.endpoint_id) })
              }
              break
            case 'UpdateStateChanged':
              // Invalidate system-info so other clients see update state changes
              queryClient.invalidateQueries({ queryKey: queryKeys.systemInfo })
              break
          }
        } catch (err) {
          console.error('Failed to parse WebSocket message:', err)
        }
      }

      ws.onclose = () => {
        setIsConnected(false)
        onDisconnectRef.current?.()
        wsRef.current = null

        // Schedule reconnection with exponential backoff
        if (shouldReconnectRef.current) {
          if (reconnectTimeoutRef.current) {
            clearTimeout(reconnectTimeoutRef.current)
          }
          scheduleReconnect()
        }
      }

      // Browser fires onclose automatically after onerror; no manual ws.close() needed
      ws.onerror = () => {
        console.warn('WebSocket connection error')
      }
    } catch (err) {
      console.error('Failed to create WebSocket:', err)
      // Schedule reconnection with exponential backoff
      scheduleReconnect()
    }
  }, [queryClient, scheduleReconnect])

  useEffect(() => {
    connectRef.current = connect
  }, [connect])

  const disconnect = useCallback(() => {
    shouldReconnectRef.current = false
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current)
      reconnectTimeoutRef.current = null
    }
    reconnectAttemptRef.current = 0
    if (wsRef.current) {
      wsRef.current.close()
      wsRef.current = null
    }
  }, [])

  useEffect(() => {
    if (!enabled) {
      disconnect()
      return
    }
    connect()
    return () => {
      disconnect()
    }
  }, [connect, disconnect, enabled])

  return {
    isConnected,
    lastEvent,
    reconnect: connect,
    disconnect,
  }
}

/**
 * Hook specifically for dashboard page
 * Connects to WebSocket and provides connection status
 */
interface UseDashboardWebSocketOptions {
  enabled?: boolean
}

export function useDashboardWebSocket(options: UseDashboardWebSocketOptions = {}) {
  const { enabled = true } = options
  const { isConnected, lastEvent, reconnect } = useWebSocket({
    enabled,
  })

  return {
    isConnected,
    lastEvent,
    reconnect,
  }
}
