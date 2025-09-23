import { describe, it, expect } from 'vitest'

describe('Partial Components', () => {
  it('should render MafsModal component', async () => {
    const { default: MafsModal } = await import('../../renderer/pages/components/partials/MafsModal')
    expect(MafsModal).toBeDefined()
    expect(typeof MafsModal).toBe('function')
  })

  it('should render UploadButton component', async () => {
    const { default: UploadButton } = await import('../../renderer/pages/components/partials/shared/UploadButton')
    expect(UploadButton).toBeDefined()
    expect(typeof UploadButton).toBe('function')
  })

  it('should render Dragable component', async () => {
    const { default: Dragable } = await import('../../renderer/pages/components/partials/timeline/Dragable')
    expect(Dragable).toBeDefined()
    expect(typeof Dragable).toBe('function')
  })

  it('should render ScaleRangeInput component', async () => {
    const { default: ScaleRangeInput } = await import('../../renderer/pages/components/partials/timeline/ScaleRangeInput')
    expect(ScaleRangeInput).toBeDefined()
    expect(typeof ScaleRangeInput).toBe('function')
  })

  it('should render SeekPlayer component', async () => {
    const { default: SeekPlayer } = await import('../../renderer/pages/components/partials/timeline/SeekPlayer')
    expect(SeekPlayer).toBeDefined()
    expect(typeof SeekPlayer).toBe('function')
  })

  it('should render Timeframe component', async () => {
    const { default: Timeframe } = await import('../../renderer/pages/components/partials/timeline/Timeframe')
    expect(Timeframe).toBeDefined()
    expect(typeof Timeframe).toBe('function')
  })
})
