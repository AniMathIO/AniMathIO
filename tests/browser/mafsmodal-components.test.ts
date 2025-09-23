import { describe, it, expect } from 'vitest'

describe('MafsModal Components', () => {
  it('should render CircleInputs component', async () => {
    const { default: CircleInputs } = await import('../../renderer/pages/components/partials/mafsmodal/CircleInputs')
    expect(CircleInputs).toBeDefined()
    expect(typeof CircleInputs).toBe('function')
  })

  it('should render EllipseInputs component', async () => {
    const { default: EllipseInputs } = await import('../../renderer/pages/components/partials/mafsmodal/EllipseInputs')
    expect(EllipseInputs).toBeDefined()
    expect(typeof EllipseInputs).toBe('function')
  })

  it('should render LatexInputs component', async () => {
    const { default: LatexInputs } = await import('../../renderer/pages/components/partials/mafsmodal/LatexInputs')
    expect(LatexInputs).toBeDefined()
    expect(typeof LatexInputs).toBe('function')
  })

  it('should render LineInputs component', async () => {
    const { default: LineInputs } = await import('../../renderer/pages/components/partials/mafsmodal/LineInputs')
    expect(LineInputs).toBeDefined()
    expect(typeof LineInputs).toBe('function')
  })

  it('should render PlotInputs component', async () => {
    const { default: PlotInputs } = await import('../../renderer/pages/components/partials/mafsmodal/PlotInputs')
    expect(PlotInputs).toBeDefined()
    expect(typeof PlotInputs).toBe('function')
  })

  it('should render PointInputs component', async () => {
    const { default: PointInputs } = await import('../../renderer/pages/components/partials/mafsmodal/PointInputs')
    expect(PointInputs).toBeDefined()
    expect(typeof PointInputs).toBe('function')
  })

  it('should render PolygonInputs component', async () => {
    const { default: PolygonInputs } = await import('../../renderer/pages/components/partials/mafsmodal/PolygonInputs')
    expect(PolygonInputs).toBeDefined()
    expect(typeof PolygonInputs).toBe('function')
  })

  it('should render TextInputs component', async () => {
    const { default: TextInputs } = await import('../../renderer/pages/components/partials/mafsmodal/TextInputs')
    expect(TextInputs).toBeDefined()
    expect(typeof TextInputs).toBe('function')
  })

  it('should render TextToLatex component', async () => {
    const { default: TextToLatex } = await import('../../renderer/pages/components/partials/mafsmodal/TextToLatex')
    expect(TextToLatex).toBeDefined()
    expect(typeof TextToLatex).toBe('function')
  })

  it('should render VectorInputs component', async () => {
    const { default: VectorInputs } = await import('../../renderer/pages/components/partials/mafsmodal/VectorInputs')
    expect(VectorInputs).toBeDefined()
    expect(typeof VectorInputs).toBe('function')
  })
})
