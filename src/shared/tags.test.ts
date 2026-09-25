import { describe, expect, it } from 'vitest'
import { normalizeTag } from './tags'

describe('normalizeTag', () => {
  it('trims, collapses whitespace and case-folds', () => {
    expect(normalizeTag('  Summer   Holidays ')).toBe('summer holidays')
  })

  it('applies NFKC so composed and decomposed forms match', () => {
    expect(normalizeTag('Cafe\u0301')).toBe(normalizeTag('Café'))
    expect(normalizeTag('ｆｕｌｌｗｉｄｔｈ')).toBe('fullwidth')
  })
})
