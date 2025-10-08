import { describe, it, expect } from 'vitest'
import { isValidComponent } from './test-utils'

describe('Panel Components', () => {
  it('should render AnimationsPanel component', async () => {
    const { default: AnimationsPanel } = await import('../../renderer/pages/components/panels/AnimationsPanel')
    expect(AnimationsPanel).toBeDefined()
    expect(isValidComponent(AnimationsPanel)).toBe(true)
  })

  it('should render AudioMixerPanel component', async () => {
    const { default: AudioMixerPanel } = await import('../../renderer/pages/components/panels/AudioMixerPanel')
    expect(AudioMixerPanel).toBeDefined()
    expect(isValidComponent(AudioMixerPanel)).toBe(true)
  })

  it('should render AudioResourcesPanel component', async () => {
    const { default: AudioResourcesPanel } = await import('../../renderer/pages/components/panels/AudioResourcesPanel')
    expect(AudioResourcesPanel).toBeDefined()
    expect(isValidComponent(AudioResourcesPanel)).toBe(true)
  })

  it('should render EffectsPanel component', async () => {
    const { default: EffectsPanel } = await import('../../renderer/pages/components/panels/EffectsPanel')
    expect(EffectsPanel).toBeDefined()
    expect(isValidComponent(EffectsPanel)).toBe(true)
  })

  it('should render ElementsPanel component', async () => {
    const { default: ElementsPanel } = await import('../../renderer/pages/components/panels/ElementsPanel')
    expect(ElementsPanel).toBeDefined()
    expect(isValidComponent(ElementsPanel)).toBe(true)
  })

  it('should render ExportVideoPanel component', async () => {
    const { default: ExportVideoPanel } = await import('../../renderer/pages/components/panels/ExportVideoPanel')
    expect(ExportVideoPanel).toBeDefined()
    expect(isValidComponent(ExportVideoPanel)).toBe(true)
  })

  it('should render FillPanel component', async () => {
    const { default: FillPanel } = await import('../../renderer/pages/components/panels/FillPanel')
    expect(FillPanel).toBeDefined()
    expect(isValidComponent(FillPanel)).toBe(true)
  })

  it('should render ImageResourcesPanel component', async () => {
    const { default: ImageResourcesPanel } = await import('../../renderer/pages/components/panels/ImageResourcesPanel')
    expect(ImageResourcesPanel).toBeDefined()
    expect(isValidComponent(ImageResourcesPanel)).toBe(true)
  })

  it('should render MafsPanel component', async () => {
    const { default: MafsPanel } = await import('../../renderer/pages/components/panels/MafsPanel')
    expect(MafsPanel).toBeDefined()
    expect(isValidComponent(MafsPanel)).toBe(true)
  })

  it('should render TextResourcesPanel component', async () => {
    const { default: TextResourcesPanel } = await import('../../renderer/pages/components/panels/TextResourcesPanel')
    expect(TextResourcesPanel).toBeDefined()
    expect(isValidComponent(TextResourcesPanel)).toBe(true)
  })

  it('should render VideoResourcesPanel component', async () => {
    const { default: VideoResourcesPanel } = await import('../../renderer/pages/components/panels/VideoResourcesPanel')
    expect(VideoResourcesPanel).toBeDefined()
    expect(isValidComponent(VideoResourcesPanel)).toBe(true)
  })
})
