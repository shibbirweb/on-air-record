/**
 * The release note parsing branches `releaseNotes.test.ts` leaves out: bold stripped from headings,
 * `*` bullets, a list interrupting a paragraph and a paragraph after a list, an indented line with no
 * list to continue, every heading depth, and inline edge cases (a link wrapping to another scheme, an
 * unclosed marker, text around a bare address).
 *
 * The result is data rendered by React, never HTML, so these check structure only.
 */

import { describe, expect, it } from 'vitest';

import { parseInline, parseNotes } from '../releaseNotes';

describe('parseNotes, remaining shapes', () => {
  it('strips bold markers from a heading', () => {
    expect(parseNotes('## **Fixed** things')).toEqual([{ kind: 'heading', text: 'Fixed things' }]);
  });

  it('reads every heading depth from one to six, and not seven', () => {
    const blocks = parseNotes(['# One', '###### Six', '####### Seven'].join('\n'));
    expect(blocks).toEqual([
      { kind: 'heading', text: 'One' },
      { kind: 'heading', text: 'Six' },
      { kind: 'paragraph', inlines: [{ kind: 'text', text: '####### Seven' }] },
    ]);
  });

  it('accepts star bullets as well as dashes', () => {
    expect(parseNotes('* one\n- two')).toEqual([
      { kind: 'list', items: [[{ kind: 'text', text: 'one' }], [{ kind: 'text', text: 'two' }]] },
    ]);
  });

  it('ends a paragraph when a list starts straight after it', () => {
    expect(parseNotes('Intro line\n- item')).toEqual([
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'Intro line' }] },
      { kind: 'list', items: [[{ kind: 'text', text: 'item' }]] },
    ]);
  });

  it('ends a list when an unindented line follows it', () => {
    expect(parseNotes('- item\nAfterwards')).toEqual([
      { kind: 'list', items: [[{ kind: 'text', text: 'item' }]] },
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'Afterwards' }] },
    ]);
  });

  it('treats an indented line with no list above as part of a paragraph', () => {
    expect(parseNotes('First\n    indented')).toEqual([
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'First indented' }] },
    ]);
  });

  it('ends a list at a heading', () => {
    expect(parseNotes('- item\n### Next')).toEqual([
      { kind: 'list', items: [[{ kind: 'text', text: 'item' }]] },
      { kind: 'heading', text: 'Next' },
    ]);
  });

  it('copes with old Mac line endings and a body of blank lines', () => {
    expect(parseNotes('# A\r- b')).toEqual([
      { kind: 'heading', text: 'A' },
      { kind: 'list', items: [[{ kind: 'text', text: 'b' }]] },
    ]);
    expect(parseNotes('\n\n   \n')).toEqual([]);
  });
});

describe('parseInline, edge cases', () => {
  it('is empty for an empty line', () => {
    expect(parseInline('')).toEqual([]);
  });

  it('keeps text on both sides of a bare address', () => {
    expect(parseInline('see https://example.com/a for more')).toEqual([
      { kind: 'text', text: 'see ' },
      { kind: 'link', text: 'https://example.com/a', href: 'https://example.com/a' },
      { kind: 'text', text: ' for more' },
    ]);
  });

  it('keeps an unclosed marker as the text it is', () => {
    expect(parseInline('a **half bold and `half code')).toEqual([
      { kind: 'text', text: 'a **half bold and `half code' },
    ]);
  });

  it('accepts a web link whatever the case of its scheme', () => {
    expect(parseInline('[docs](HTTPS://example.com)')).toEqual([
      { kind: 'link', text: 'docs', href: 'HTTPS://example.com' },
    ]);
  });

  it('keeps only the words of a link to a javascript or mail target', () => {
    expect(parseInline('[click](javascript:alert(1))')[0]).toEqual({ kind: 'text', text: 'click' });
    expect(parseInline('[mail](mailto:a@b.c)')).toEqual([{ kind: 'text', text: 'mail' }]);
  });

  it('reads several constructs in a row', () => {
    expect(parseInline('**a**`b`[c](http://d.e)')).toEqual([
      { kind: 'bold', text: 'a' },
      { kind: 'code', text: 'b' },
      { kind: 'link', text: 'c', href: 'http://d.e' },
    ]);
  });
});
