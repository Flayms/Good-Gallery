import { describe, expect, it } from 'vitest'
import { accumulateWheel, WHEEL_STEP } from './use-ctrl-wheel-zoom'

describe('accumulateWheel', () => {
  it('turns a wheel notch into one step', () => {
    expect(accumulateWheel(0, WHEEL_STEP)).toEqual({ steps: 1, rest: 0 })
    expect(accumulateWheel(0, -WHEEL_STEP)).toEqual({ steps: -1, rest: 0 })
  })

  it('collects small touchpad deltas until they add up to a step', () => {
    let accumulated = 0
    const steps: number[] = []
    for (let i = 0; i < 10; i++) {
      const result = accumulateWheel(accumulated, 30)
      accumulated = result.rest
      steps.push(result.steps)
    }
    expect(steps.reduce((sum, step) => sum + step, 0)).toBe(3)
    expect(accumulated).toBe(0)
  })

  it('splits large deltas into several steps', () => {
    expect(accumulateWheel(0, 250)).toEqual({ steps: 2, rest: 50 })
  })

  it('drops the remainder when the direction changes', () => {
    expect(accumulateWheel(90, -20)).toEqual({ steps: 0, rest: -20 })
  })
})
