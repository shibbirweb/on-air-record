/**
 * `cn`, the class name helper every component uses. What matters is the promise in its comment: a
 * caller's Tailwind class beats a component default for the same property, whatever order the stylesheet
 * happens to define them in, while unrelated classes and conditional ones pass through.
 */

import { describe, expect, it } from 'vitest';

import { cn } from '../utils';

describe('cn', () => {
  it('lets a later class win over an earlier one for the same property', () => {
    expect(cn('px-2 py-1', 'px-4')).toBe('py-1 px-4');
    expect(cn('text-sm text-muted-foreground', 'text-destructive')).toBe('text-sm text-destructive');
  });

  it('keeps classes for different properties side by side', () => {
    expect(cn('flex', 'items-center', 'gap-2')).toBe('flex items-center gap-2');
  });

  it('drops falsy entries and honours conditional objects and arrays', () => {
    const disabled = false;
    expect(cn('base', disabled && 'opacity-50', null, undefined, { 'font-bold': true, italic: false }, [
      'rounded',
      ['shadow'],
    ])).toBe('base font-bold rounded shadow');
  });

  it('is empty for nothing', () => {
    expect(cn()).toBe('');
    expect(cn(false, null, '')).toBe('');
  });
});
