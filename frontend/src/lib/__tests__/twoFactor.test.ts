import { describe, expect, it } from 'vitest';

import { recoveryCodesFile, svgDataUri } from '../twoFactor';

describe('svgDataUri', () => {
  it('makes an image URL that keeps every character of the document', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0h1v1H0z" fill="#000"/></svg>';
    const uri = svgDataUri(svg);
    expect(uri.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true);
    expect(decodeURIComponent(uri.slice(uri.indexOf(',') + 1))).toBe(svg);
    // Nothing that could end the attribute it sits in survives unencoded.
    expect(uri).not.toMatch(/["<>#]/);
  });
});

describe('recoveryCodesFile', () => {
  it('names the account and the date, then lists every code on its own line', () => {
    const text = recoveryCodesFile(['abcde-fghjk', 'mnpqr-stuvw'], 'owner@example.com', new Date('2026-09-26T10:00:00Z'));
    const lines = text.split('\n');
    expect(lines).toContain('Account: owner@example.com');
    expect(lines).toContain('Created: 2026-09-26');
    expect(lines).toContain('abcde-fghjk');
    expect(lines).toContain('mnpqr-stuvw');
  });
});
