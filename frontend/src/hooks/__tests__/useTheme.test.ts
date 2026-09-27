// @vitest-environment jsdom
/**
 * `useTheme` and `applyPreferredTheme`: choosing light or dark, putting it on the page, and remembering it.
 *
 * The choice comes from storage first, then the system preference, then dark. Storage can be blocked and
 * `matchMedia` can be missing (jsdom has none), and neither may stop the page rendering, so both failures
 * are driven explicitly alongside the normal paths.
 */

import '@/test/dom';

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { applyPreferredTheme, useTheme } from '../useTheme';

const KEY = 'oar-theme';

function prefersLight(light: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn((query: string) => ({ matches: light && query === '(prefers-color-scheme: light)' })),
  });
}

function root() {
  return document.documentElement;
}

beforeEach(() => {
  window.localStorage.clear();
  root().classList.remove('dark');
  root().style.colorScheme = '';
  Reflect.deleteProperty(window, 'matchMedia');
});

afterEach(() => {
  vi.restoreAllMocks();
  Reflect.deleteProperty(window, 'matchMedia');
});

describe('applyPreferredTheme', () => {
  it('uses the stored choice ahead of the system preference', () => {
    window.localStorage.setItem(KEY, 'light');
    prefersLight(false);
    applyPreferredTheme();
    expect(root()).not.toHaveClass('dark');
    expect(root().style.colorScheme).toBe('light');
  });

  it('follows the system when nothing is stored', () => {
    prefersLight(true);
    applyPreferredTheme();
    expect(root()).not.toHaveClass('dark');
    expect(root().style.colorScheme).toBe('light');
  });

  it('defaults to dark when the system has no light preference', () => {
    prefersLight(false);
    applyPreferredTheme();
    expect(root()).toHaveClass('dark');
    expect(root().style.colorScheme).toBe('dark');
  });

  it('defaults to dark where matchMedia does not exist', () => {
    applyPreferredTheme();
    expect(root()).toHaveClass('dark');
  });

  it('ignores a stored value that is not a theme', () => {
    window.localStorage.setItem(KEY, 'purple');
    prefersLight(true);
    applyPreferredTheme();
    expect(root().style.colorScheme).toBe('light');
  });

  it('falls back to the system preference when storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('SecurityError');
    });
    prefersLight(true);
    expect(() => applyPreferredTheme()).not.toThrow();
    expect(root().style.colorScheme).toBe('light');
  });
});

describe('useTheme', () => {
  it('starts from the stored choice and puts it on the page', () => {
    window.localStorage.setItem(KEY, 'light');
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe('light');
    expect(root()).not.toHaveClass('dark');
  });

  it('toggles between dark and light, updating the page and storage each time', () => {
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe('dark');
    expect(window.localStorage.getItem(KEY)).toBe('dark');

    act(() => {
      result.current.toggleTheme();
    });
    expect(result.current.theme).toBe('light');
    expect(root()).not.toHaveClass('dark');
    expect(root().style.colorScheme).toBe('light');
    expect(window.localStorage.getItem(KEY)).toBe('light');

    act(() => {
      result.current.toggleTheme();
    });
    expect(result.current.theme).toBe('dark');
    expect(root()).toHaveClass('dark');
    expect(window.localStorage.getItem(KEY)).toBe('dark');
  });

  it('keeps the same toggle function across renders', () => {
    const { result, rerender } = renderHook(() => useTheme());
    const first = result.current.toggleTheme;
    rerender();
    expect(result.current.toggleTheme).toBe(first);
  });

  it('still switches the page when the choice cannot be stored', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError');
    });
    const { result } = renderHook(() => useTheme());
    act(() => {
      result.current.toggleTheme();
    });
    expect(result.current.theme).toBe('light');
    expect(root().style.colorScheme).toBe('light');
  });
});
