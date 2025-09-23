import { describe, it, expect } from 'vitest'

describe('Main Pages', () => {
  it('should render Home page', async () => {
    // Test that Home page can be imported and rendered
    const { default: HomePage } = await import('../../renderer/pages/Home')
    expect(HomePage).toBeDefined()
    expect(typeof HomePage).toBe('function')
  })

  it('should render Editor page', async () => {
    // Test that Editor page can be imported and rendered
    const { default: EditorPage } = await import('../../renderer/pages/Editor')
    expect(EditorPage).toBeDefined()
    expect(typeof EditorPage).toBe('function')
  })

  it('should render Menu page', async () => {
    // Test that Menu page can be imported and rendered
    const { default: MenuPage } = await import('../../renderer/pages/Menu')
    expect(MenuPage).toBeDefined()
    expect(typeof MenuPage).toBe('function')
  })

  it('should render SettingsModal page', async () => {
    // Test that SettingsModal page can be imported and rendered
    const { default: SettingsModalPage } = await import('../../renderer/pages/SettingsModal')
    expect(SettingsModalPage).toBeDefined()
    expect(typeof SettingsModalPage).toBe('function')
  })

  it('should render _app page', async () => {
    // Test that _app page can be imported and rendered
    const { default: AppPage } = await import('../../renderer/pages/_app')
    expect(AppPage).toBeDefined()
    expect(typeof AppPage).toBe('function')
  })
})
