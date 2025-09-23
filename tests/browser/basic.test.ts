import { describe, it, expect } from 'vitest'

describe('Browser Tests', () => {
  it('should run in browser environment', () => {
    expect(typeof window).toBe('object')
    expect(typeof document).toBe('object')
  })

  it('should have DOM APIs available', () => {
    const div = document.createElement('div')
    div.textContent = 'Test'
    expect(div.textContent).toBe('Test')
  })

  it('should be able to manipulate DOM', () => {
    const body = document.body
    expect(body).toBeDefined()
    
    const testDiv = document.createElement('div')
    testDiv.id = 'test-element'
    body.appendChild(testDiv)
    
    const foundElement = document.getElementById('test-element')
    expect(foundElement).toBe(testDiv)
    
    body.removeChild(testDiv)
  })
})
