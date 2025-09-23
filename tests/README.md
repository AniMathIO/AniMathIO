# AniMathIO Testing Suite

This directory contains comprehensive tests for the AniMathIO Electron application, including both unit tests and UI tests.

## Test Structure

```
tests/
├── README.md                    # This file
├── run-all-tests.js            # Combined test runner
├── setup.ts                    # Vitest setup configuration
├── playwright/                 # Playwright UI tests
│   ├── screenshots/           # Test screenshots
│   ├── electron-app.spec.ts   # Basic Electron app tests
│   ├── ui-components.spec.ts   # UI component tests
│   └── state-management.spec.ts # State management tests
├── renderer/                   # Vitest unit tests
│   ├── states/                # State management tests
│   ├── utils/                 # Utility function tests
│   └── pages/                 # Page component tests
└── main/                      # Main process tests
```

## Test Types

### 1. Unit Tests (Vitest)
- **Purpose**: Test business logic, utilities, and state management in isolation
- **Coverage Goal**: High coverage for core business logic
- **Location**: `tests/renderer/states/`, `tests/renderer/utils/`, and `tests/main/`
- **Run**: `npm run test:coverage`

### 2. Browser Tests (Vitest + Playwright)
- **Purpose**: Test user interactions and browser functionality
- **Coverage Goal**: Comprehensive browser interaction testing
- **Location**: `tests/browser/`
- **Run**: `npm run test:browser`

## Running Tests

### Individual Test Suites

```bash
# Run unit tests with coverage
npm run test:coverage

# Run unit tests in watch mode
npm run test:watch

# Run unit tests with UI
npm run test:ui

# Run browser tests
npm run test:browser

# Run browser tests with UI
npm run test:browser:ui
```

### Combined Test Suite

```bash
# Run all tests with comprehensive reporting
npm run test:all

# Simple combined run
npm run test:all:simple
```

## Test Configuration

### Vitest Configuration
- **File**: `vitest.config.mts`
- **Environment**: `happy-dom`
- **Coverage**: V8 provider with multiple reporters
- **Thresholds**: Different for UI vs non-UI components

### Browser Testing Configuration
- **Provider**: Playwright via Vitest
- **Target**: Web browsers (Chromium)
- **Features**: Headless testing, screenshots, traces

## Coverage Goals

### Unit Tests (Vitest) - Business Logic Focus
- ✅ `renderer/states/` - State management logic (49.4% coverage)
- ✅ `renderer/utils/` - Utility functions (100% for index.ts, 23% for fabric-utils.ts)
- ✅ `main/` - Electron main process (0% - complex to test)

### UI Tests (Playwright) - User Interaction Focus
- ✅ React component rendering
- ✅ User interactions
- ✅ Electron app lifecycle
- ✅ Canvas interactions
- ✅ Menu navigation

## Test Reports

After running tests, you can view detailed reports:

- **Coverage Report**: `file://coverage/index.html`
- **Playwright Report**: `file://playwright-report/index.html`
- **Screenshots**: `tests/playwright/screenshots/`

## Writing New Tests

### Unit Tests (Vitest)
```typescript
import { describe, it, expect } from 'vitest';

describe('State Management', () => {
  it('should handle state updates correctly', () => {
    const state = new State();
    expect(state.selectedMenuOption).toBe('home');
  });
});
```

### Browser Tests (Vitest + Playwright)
```typescript
import { describe, it, expect } from 'vitest'
import { page } from '@vitest/browser/context'

describe('Browser Tests', () => {
  it('should interact with UI', async () => {
    await page.goto('data:text/html,<button>Click Me</button>')
    
    const button = page.locator('button')
    await expect(button).toBeVisible()
    await button.click()
  });
});
```

## Best Practices

1. **Unit Tests**: Focus on business logic, state management, and utility functions
2. **Browser Tests**: Test user interactions and browser functionality
3. **Separation**: Keep unit tests separate from browser tests
4. **Mocking**: Use appropriate mocks for external dependencies
5. **Coverage**: Aim for high coverage on critical business logic
6. **Performance**: Keep tests fast and reliable

## Troubleshooting

### Common Issues

1. **Electron not launching**: Check if the app builds correctly
2. **Playwright timeouts**: Increase timeout in test configuration
3. **Coverage not meeting thresholds**: Add more test cases
4. **Mock issues**: Ensure proper mocking of external dependencies

### Debug Mode

```bash
# Run with debug output
DEBUG=* npm run test:all

# Run Playwright in headed mode to see what's happening
npm run test:playwright:headed
```

## Continuous Integration

The test suite is designed to work in CI environments:

```yaml
# Example GitHub Actions
- name: Run Tests
  run: npm run test:all
```

## Contributing

When adding new features:

1. Write unit tests for business logic
2. Write UI tests for user interactions
3. Ensure coverage thresholds are met
4. Update this README if needed
