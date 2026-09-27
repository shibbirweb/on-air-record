/**
 * Shared setup for component tests. They run in jsdom, each opting in with `// @vitest-environment jsdom`
 * at the top, while the rest of the suite stays in the faster node environment.
 *
 * jsdom is a document, not a browser: it has no layout, no ResizeObserver and no pointer capture, which
 * the component library's slider and dropdown expect, so those are stood in for here. Anything that needs
 * a real browser to mean something (drawing that is actually seen, real pointer input) is covered by
 * `scripts/e2e.mjs` in headless Chrome instead.
 */

import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
});

if (!('ResizeObserver' in globalThis)) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

Element.prototype.hasPointerCapture ??= () => false;
Element.prototype.setPointerCapture ??= () => {};
Element.prototype.releasePointerCapture ??= () => {};
Element.prototype.scrollIntoView ??= () => {};
