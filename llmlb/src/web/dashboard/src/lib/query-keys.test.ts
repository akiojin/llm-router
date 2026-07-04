import { describe, it, expect } from 'vitest'
import { queryKeys } from './query-keys'

describe('queryKeys 定数', () => {
  it('固定キーは安定したタプルを返す', () => {
    expect(queryKeys.dashboardEndpoints).toEqual(['dashboard-endpoints'])
    expect(queryKeys.models).toEqual(['models'])
    expect(queryKeys.users).toEqual(['users'])
    expect(queryKeys.apiKeys).toEqual(['api-keys'])
  })
})

describe('queryKeys ファクトリ', () => {
  it('引数を末尾要素に含めたタプルを生成する', () => {
    expect(queryKeys.endpoint('e1')).toEqual(['endpoint', 'e1'])
    expect(queryKeys.endpointModelTps('e1')).toEqual(['endpoint-model-tps', 'e1'])
    expect(queryKeys.endpointDailyStats('e1', 7)).toEqual(['endpoint-daily-stats', 'e1', 7])
    expect(queryKeys.clientRanking(1, 20, 'ip')).toEqual(['client-ranking', 1, 20, 'ip'])
  })

  it('プレフィックス無効化キーは client-ranking ファクトリの先頭要素と一致する', () => {
    const ranking = queryKeys.clientRanking(2, 10, null)
    expect(ranking[0]).toBe(queryKeys.clientRankingPrefix[0])
  })

  it('異なる引数は異なるキーを生成する', () => {
    expect(queryKeys.endpoint('a')).not.toEqual(queryKeys.endpoint('b'))
  })
})
