import { describe, it, expect } from 'vitest'
import {
  getStatusBadgeVariant,
  getStatusLabel,
  getStatusIndicatorColor,
  getTypeLabel,
  getTypeBadgeVariant,
} from './endpoint-display'

describe('getStatusBadgeVariant', () => {
  it('既知の status を対応する variant へ変換する', () => {
    expect(getStatusBadgeVariant('online')).toBe('online')
    expect(getStatusBadgeVariant('pending')).toBe('pending')
    expect(getStatusBadgeVariant('offline')).toBe('offline')
    expect(getStatusBadgeVariant('error')).toBe('destructive')
  })

  it('undefined は outline へフォールバックする', () => {
    expect(getStatusBadgeVariant(undefined)).toBe('outline')
  })
})

describe('getStatusLabel', () => {
  it('既知の status を表示ラベルへ変換する', () => {
    expect(getStatusLabel('online')).toBe('Online')
    expect(getStatusLabel('pending')).toBe('Pending')
    expect(getStatusLabel('offline')).toBe('Offline')
    expect(getStatusLabel('error')).toBe('Error')
  })

  it('undefined は Unknown へフォールバックする', () => {
    expect(getStatusLabel(undefined)).toBe('Unknown')
  })
})

describe('getStatusIndicatorColor', () => {
  it('status に応じたテキスト色クラスを返す', () => {
    expect(getStatusIndicatorColor('online')).toBe('text-success')
    expect(getStatusIndicatorColor('pending')).toBe('text-warning')
    expect(getStatusIndicatorColor('offline')).toBe('text-destructive/70')
    expect(getStatusIndicatorColor('error')).toBe('text-destructive')
  })

  it('undefined は muted-foreground へフォールバックする', () => {
    expect(getStatusIndicatorColor(undefined)).toBe('text-muted-foreground')
  })
})

describe('getTypeLabel', () => {
  it('既知の type を表示名へ変換する', () => {
    expect(getTypeLabel('xllm')).toBe('xLLM')
    expect(getTypeLabel('ollama')).toBe('Ollama')
    expect(getTypeLabel('vllm')).toBe('vLLM')
    expect(getTypeLabel('lm_studio')).toBe('LM Studio')
    expect(getTypeLabel('llamacpp')).toBe('llama.cpp')
    expect(getTypeLabel('openai_compatible')).toBe('OpenAI Compatible')
    expect(getTypeLabel('unknown')).toBe('Unknown')
  })

  it('undefined は "-" へフォールバックする', () => {
    expect(getTypeLabel(undefined)).toBe('-')
  })
})

describe('getTypeBadgeVariant', () => {
  it('xLLM は default バッジを返す', () => {
    expect(getTypeBadgeVariant('xllm')).toBe('default')
  })

  it('外部推論エンジンは secondary バッジを返す', () => {
    expect(getTypeBadgeVariant('ollama')).toBe('secondary')
    expect(getTypeBadgeVariant('vllm')).toBe('secondary')
    expect(getTypeBadgeVariant('lm_studio')).toBe('secondary')
    expect(getTypeBadgeVariant('llamacpp')).toBe('secondary')
  })

  it('未知/汎用および undefined は outline バッジを返す', () => {
    expect(getTypeBadgeVariant('openai_compatible')).toBe('outline')
    expect(getTypeBadgeVariant('unknown')).toBe('outline')
    expect(getTypeBadgeVariant(undefined)).toBe('outline')
  })
})
