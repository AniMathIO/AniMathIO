import { describe, it, expect } from 'vitest'
import { isValidComponent } from './test-utils.ts'

describe('Panel Components', () => {
  it('should render AnimationsPanel component', async () => {
    const { default: AnimationsPanel } = await import('../../renderer/pages/components/panels/AnimationsPanel')
    expect(AnimationsPanel).toBeDefined()
    expect(typeof AnimationsPanel).toBe('function')
  })

  it('should render AudioMixerPanel component', async () => {
    const { default: AudioMixerPanel } = await import('../../renderer/pages/components/panels/AudioMixerPanel')
    expect(AudioMixerPanel).toBeDefined()
    expect(typeof AudioMixerPanel).toBe('function')
  })

  it('should render AudioResourcesPanel component', async () => {
    const { default: AudioResourcesPanel } = await import('../../renderer/pages/components/panels/AudioResourcesPanel')
    expect(AudioResourcesPanel).toBeDefined()
    expect(typeof AudioResourcesPanel).toBe('function')
  })

  it('should render EffectsPanel component', async () => {
    const { default: EffectsPanel } = await import('../../renderer/pages/components/panels/EffectsPanel')
    expect(EffectsPanel).toBeDefined()
    expect(typeof EffectsPanel).toBe('function')
  })

  it('should render ElementsPanel component', async () => {
    const { default: ElementsPanel } = await import('../../renderer/pages/components/panels/ElementsPanel')
    expect(ElementsPanel).toBeDefined()
    expect(typeof ElementsPanel).toBe('function')
  })

  it('should render ExportVideoPanel component', async () => {
    const { default: ExportVideoPanel } = await import('../../renderer/pages/components/panels/ExportVideoPanel')
    expect(ExportVideoPanel).toBeDefined()
    expect(typeof ExportVideoPanel).toBe('function')
  })

  it('should render FillPanel component', async () => {
    const { default: FillPanel } = await import('../../renderer/pages/components/panels/FillPanel')
    expect(FillPanel).toBeDefined()
    expect(typeof FillPanel).toBe('function')
  })

  it('should render ImageResourcesPanel component', async () => {
    const { default: ImageResourcesPanel } = await import('../../renderer/pages/components/panels/ImageResourcesPanel')
    expect(ImageResourcesPanel).toBeDefined()
    expect(typeof ImageResourcesPanel).toBe('function')
  })

  it('should render MafsPanel component', async () => {
    const { default: MafsPanel } = await import('../../renderer/pages/components/panels/MafsPanel')
    expect(MafsPanel).toBeDefined()
    expect(typeof MafsPanel).toBe('function')
  })

  it('should render TextResourcesPanel component', async () => {
    const { default: TextResourcesPanel } = await import('../../renderer/pages/components/panels/TextResourcesPanel')
    expect(TextResourcesPanel).toBeDefined()
    expect(typeof TextResourcesPanel).toBe('function')
  })

  it('should render VideoResourcesPanel component', async () => {
    const { default: VideoResourcesPanel } = await import('../../renderer/pages/components/panels/VideoResourcesPanel')
    expect(VideoResourcesPanel).toBeDefined()
    expect(typeof VideoResourcesPanel).toBe('function')
  })
})
