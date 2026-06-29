// アシスタント応答の content（最終回答）と reasoning（思考過程）を分離するヘルパー。
//
// LM Studio / vLLM 等の推論モデルは OpenAI 互換ストリームで思考過程を
// `delta.reasoning_content`（または `delta.reasoning`）として content とは別フィールドで
// 返す。これらを content に混ぜると最終回答が読みづらくなるため、受信時点で分離して
// 別チャネルとして保持し、UI 側で「思考過程」を折りたたみ表示する。

export interface AssistantTextParts {
  /** 最終回答（吹き出しの主表示） */
  content: string
  /** 思考過程（折りたたみ表示。未提供なら空文字） */
  reasoning: string
}

interface DeltaChoice {
  delta?: {
    content?: string
    reasoning?: string
    reasoning_content?: string
  }
}

interface MessageChoice {
  message?: {
    content?: string
    reasoning?: string
    reasoning_content?: string
  }
}

/**
 * ストリーミングの 1 チャンク（delta）から content と reasoning を分離する。
 *
 * content は `?? ''`（nullish）で扱う点が重要: reasoning フェーズでは `delta.content` が
 * 空文字で `reasoning_content` にトークンが届くため、空文字の content を reasoning で
 * 上書きしてはならない。
 */
export function splitAssistantDelta(parsed: { choices?: DeltaChoice[] }): AssistantTextParts {
  const delta = parsed.choices?.[0]?.delta
  return {
    content: delta?.content ?? '',
    reasoning: delta?.reasoning_content ?? delta?.reasoning ?? '',
  }
}

/** 非ストリーミング応答（message）から content と reasoning を分離する。 */
export function splitAssistantMessage(data: { choices?: MessageChoice[] }): AssistantTextParts {
  const message = data.choices?.[0]?.message
  return {
    content: message?.content ?? '',
    reasoning: message?.reasoning_content ?? message?.reasoning ?? '',
  }
}
