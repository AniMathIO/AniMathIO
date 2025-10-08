import { describe, it, expect } from 'vitest'
import { isValidComponent } from './test-utils'

describe('Main Components', () => {
  it('should render Resources component', async () => {
    const { default: Resources } = await import('../../renderer/pages/components/Resources')
    expect(Resources).toBeDefined()
    expect(isValidComponent(Resources)).toBe(true)
  })

  it('should render Timeline component', async () => {
    const { default: Timeline } = await import('../../renderer/pages/components/Timeline')
    expect(Timeline).toBeDefined()
    expect(isValidComponent(Timeline)).toBe(true)
  })
})
