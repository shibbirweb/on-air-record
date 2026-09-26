import { describe, expect, it } from 'vitest';

import { parseInline, parseNotes } from '../releaseNotes';

describe('parseNotes', () => {
  it('reads a changelog section the way the releases carry it', () => {
    const notes = [
      '',
      '### Added',
      '',
      '- **Keeps playing with the screen off.** On a phone, the broadcast now carries on when the screen',
      '  turns off. (OAR-84)',
      '- A second entry.',
      '',
      '### Changed',
      '',
      'A paragraph',
      'that wraps.',
    ].join('\n');

    expect(parseNotes(notes)).toEqual([
      { kind: 'heading', text: 'Added' },
      {
        kind: 'list',
        items: [
          [
            { kind: 'bold', text: 'Keeps playing with the screen off.' },
            {
              kind: 'text',
              text: ' On a phone, the broadcast now carries on when the screen turns off. (OAR-84)',
            },
          ],
          [{ kind: 'text', text: 'A second entry.' }],
        ],
      },
      { kind: 'heading', text: 'Changed' },
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'A paragraph that wraps.' }] },
    ]);
  });

  it('copes with Windows line endings and an empty body', () => {
    expect(parseNotes('### Fixed\r\n\r\n- One\r\n')).toEqual([
      { kind: 'heading', text: 'Fixed' },
      { kind: 'list', items: [[{ kind: 'text', text: 'One' }]] },
    ]);
    expect(parseNotes('')).toEqual([]);
  });

  it('never produces markup from what the release says', () => {
    const blocks = parseNotes('<script>alert(1)</script>');
    expect(blocks).toEqual([
      { kind: 'paragraph', inlines: [{ kind: 'text', text: '<script>alert(1)</script>' }] },
    ]);
  });
});

describe('parseInline', () => {
  it('finds bold, code and web links', () => {
    expect(parseInline('Run `make check` and see [the guide](https://example.com/guide), **now**.')).toEqual([
      { kind: 'text', text: 'Run ' },
      { kind: 'code', text: 'make check' },
      { kind: 'text', text: ' and see ' },
      { kind: 'link', text: 'the guide', href: 'https://example.com/guide' },
      { kind: 'text', text: ', ' },
      { kind: 'bold', text: 'now' },
      { kind: 'text', text: '.' },
    ]);
  });

  it('links bare web addresses without their trailing punctuation', () => {
    expect(parseInline('Merged in https://github.com/o/r/pull/16. Thanks')).toEqual([
      { kind: 'text', text: 'Merged in ' },
      { kind: 'link', text: 'https://github.com/o/r/pull/16', href: 'https://github.com/o/r/pull/16' },
      { kind: 'text', text: '. Thanks' },
    ]);
    expect(
      parseInline('compare/v0.5.0...v0.6.0 at https://github.com/o/r/compare/v0.5.0...v0.6.0')[1],
    ).toEqual({
      kind: 'link',
      text: 'https://github.com/o/r/compare/v0.5.0...v0.6.0',
      href: 'https://github.com/o/r/compare/v0.5.0...v0.6.0',
    });
  });

  it('keeps the words of a link that is not to the web, and drops the target', () => {
    expect(parseInline('[click](javascript:alert(1))')).toEqual([
      { kind: 'text', text: 'click' },
      { kind: 'text', text: ')' },
    ]);
    expect(parseInline('[setup](docs/SETUP.md)')).toEqual([{ kind: 'text', text: 'setup' }]);
  });
});
