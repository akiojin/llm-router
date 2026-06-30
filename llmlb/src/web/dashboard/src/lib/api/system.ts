// System API (self-update)

import { fetchWithAuth } from './client'

export type UpdatePayloadState =
  | { payload: 'not_ready' }
  | {
      payload: 'downloading'
      started_at: string
      downloaded_bytes?: number | null
      total_bytes?: number | null
    }
  | { payload: 'ready'; kind: unknown }
  | { payload: 'error'; message: string }

export type UpdateState =
  | { state: 'up_to_date'; checked_at?: string | null }
  | {
      state: 'available'
      current: string
      latest: string
      release_url: string
      portable_asset_url?: string | null
      installer_asset_url?: string | null
      payload: UpdatePayloadState
      checked_at: string
    }
  | {
      state: 'draining'
      latest: string
      in_flight: number
      requested_at: string
      timeout_at: string
    }
  | {
      state: 'applying'
      latest: string
      method: string
      phase?:
        | 'starting'
        | 'waiting_old_process_exit'
        | 'running_installer'
        | 'restarting'
      phase_message?: string
      started_at?: string
      timeout_at?: string
    }
  | {
      state: 'failed'
      latest?: string | null
      release_url?: string | null
      message: string
      failed_at: string
    }

export interface ScheduleInfo {
  mode: 'immediate' | 'idle' | 'scheduled'
  scheduled_at?: string | null
  scheduled_by: string
  target_version: string
  created_at: string
}

export interface SystemInfo {
  version: string
  pid: number
  in_flight: number
  update: UpdateState
  schedule?: ScheduleInfo | null
  rollback_available?: boolean
}

export interface ApplyUpdateResponse {
  queued: boolean
  mode: 'normal'
}

export interface ForceApplyUpdateResponse {
  queued: false
  mode: 'force'
  dropped_in_flight: number
}

export interface CreateScheduleRequest {
  mode: 'idle' | 'scheduled'
  scheduled_at?: string
}

export interface RollbackResponse {
  rolling_back: true
}

export interface VersionResponse {
  version: string
}

export const systemApi = {
  /** GET /api/version (認証不要) */
  getVersion: async (): Promise<VersionResponse> => {
    const res = await fetch('/api/version')
    if (!res.ok) throw new Error(`Failed to fetch version: ${res.status}`)
    return res.json()
  },
  getSystem: () => fetchWithAuth<SystemInfo>('/api/system'),
  checkUpdate: () =>
    fetchWithAuth<{ update: UpdateState }>('/api/system/update/check', {
      method: 'POST',
      body: JSON.stringify({}),
    }),
  applyUpdate: () =>
    fetchWithAuth<ApplyUpdateResponse>('/api/system/update/apply', {
      method: 'POST',
      body: JSON.stringify({}),
    }),
  applyForceUpdate: () =>
    fetchWithAuth<ForceApplyUpdateResponse>('/api/system/update/apply/force', {
      method: 'POST',
      body: JSON.stringify({}),
    }),
  createSchedule: (req: CreateScheduleRequest) =>
    fetchWithAuth<{ schedule: ScheduleInfo }>('/api/system/update/schedule', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  cancelSchedule: () =>
    fetchWithAuth<{ cancelled: true }>('/api/system/update/schedule', {
      method: 'DELETE',
    }),
  rollback: () =>
    fetchWithAuth<RollbackResponse>('/api/system/update/rollback', {
      method: 'POST',
      body: JSON.stringify({}),
    }),
}
