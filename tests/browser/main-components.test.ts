import { describe, it, expect } from 'vitest'

describe('Main Components', () => {
  it('should render Resources component', async () => {
    const { default: Resources } = await import('../../renderer/pages/components/Resources')
    expect(Resources).toBeDefined()
    expect(typeof Resources).toBe('function')
  })

  it('should render Timeline component', async () => {
    const { default: Timeline } = await import('../../renderer/pages/components/Timeline')
    expect(Timeline).toBeDefined()
    expect(typeof Timeline).toBe('function')
  })
})
