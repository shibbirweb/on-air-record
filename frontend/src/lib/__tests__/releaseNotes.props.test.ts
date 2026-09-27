/**
 * Properties of the release note parser over generated text.
 *
 * Release bodies come from GitHub, written by people and by GitHub's own generator, so the parser meets
 * whatever anybody typed. The promises checked here hold for any string, not only the changelog's shapes:
 * it never throws, what it returns is data of the documented kinds and nothing else, every link it keeps
 * points at the web, it invents no text that was not in the input, and line endings or trailing spaces do
 * not change the result. Markup such as `<script>` survives only as the plain text React escapes.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { parseInline, parseNotes, type Inline, type NoteBlock } from '../releaseNotes';

/**
 * Text built from the fragments the parser reacts to, so generated input exercises headings, bullets,
 * emphasis, code, links of every scheme and embedded markup far more often than random characters would.
 */
const markdownish = fc
  .array(
    fc.oneof(
      { weight: 3, arbitrary: fc.string({ maxLength: 6 }) },
      {
        weight: 5,
        arbitrary: fc.constantFrom(
          '# ', '## ', '###### ', '####### ', '- ', '* ', '  ', '\t', '\n', '\r\n', '\r', '\n\n',
          '**', '`', '[', ']', '(', ')', '](', 'http://', 'https://', 'HTTPS://', 'javascript:', 'data:',
          'mailto:', 'x.y', '.', ',', '<script>alert(1)</script>', '<img src=x onerror=alert(1)>', '&amp;',
          '&lt;', '"', "'", 'word',
        ),
      },
    ),
    { maxLength: 40 },
  )
  .map((parts) => parts.join(''));

const anyText = fc.oneof(markdownish, fc.string({ unit: 'binary', maxLength: 80 }));

function inlinesOf(blocks: NoteBlock[]): Inline[] {
  return blocks.flatMap((block) => {
    if (block.kind === 'paragraph') {
      return block.inlines;
    }
    if (block.kind === 'list') {
      return block.items.flat();
    }
    return [];
  });
}

/** Whether `needle` can be read out of `haystack` by deleting characters, keeping order. */
function isSubsequence(needle: string, haystack: string): boolean {
  let position = 0;
  for (const character of needle) {
    position = haystack.indexOf(character, position);
    if (position === -1) {
      return false;
    }
    position += character.length;
  }
  return true;
}

describe('parseNotes', () => {
  it('never throws, and returns only the documented block and inline shapes', () => {
    fc.assert(
      fc.property(anyText, (markdown) => {
        const blocks = parseNotes(markdown);
        for (const block of blocks) {
          if (block.kind === 'heading') {
            expect(Object.keys(block).sort()).toEqual(['kind', 'text']);
            expect(typeof block.text).toBe('string');
          } else if (block.kind === 'paragraph') {
            expect(Object.keys(block).sort()).toEqual(['inlines', 'kind']);
            expect(block.inlines.length).toBeGreaterThan(0);
          } else {
            expect(block.kind).toBe('list');
            expect(Object.keys(block).sort()).toEqual(['items', 'kind']);
            for (const item of block.items) {
              expect(item.length).toBeGreaterThan(0);
            }
          }
        }
        for (const inline of inlinesOf(blocks)) {
          expect(['text', 'bold', 'code', 'link']).toContain(inline.kind);
          const keys = inline.kind === 'link' ? ['href', 'kind', 'text'] : ['kind', 'text'];
          expect(Object.keys(inline).sort()).toEqual(keys);
          expect(inline.text.length).toBeGreaterThan(0);
        }
      }),
    );
  });

  it('keeps a link only when it points at http or https', () => {
    fc.assert(
      fc.property(anyText, (markdown) => {
        for (const inline of inlinesOf(parseNotes(markdown))) {
          if (inline.kind === 'link') {
            expect(inline.href).toMatch(/^https?:\/\//i);
            expect(inline.href).not.toMatch(/\s/);
          }
        }
      }),
    );
  });

  it('never leaves bold markers in a heading, or whitespace around it', () => {
    fc.assert(
      fc.property(anyText, (markdown) => {
        for (const block of parseNotes(markdown)) {
          if (block.kind === 'heading') {
            expect(block.text).not.toContain('**');
            expect(block.text).toBe(block.text.trim());
          }
        }
      }),
    );
  });

  it('reads the same whatever the line endings', () => {
    fc.assert(
      fc.property(fc.array(markdownish.map((text) => text.replace(/[\r\n]/g, '')), { maxLength: 12 }), (lines) => {
        const unix = parseNotes(lines.join('\n'));
        expect(parseNotes(lines.join('\r\n'))).toEqual(unix);
        expect(parseNotes(lines.join('\r'))).toEqual(unix);
      }),
    );
  });

  it('ignores trailing spaces and tabs on any line', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.tuple(markdownish.map((text) => text.replace(/[\r\n]/g, '')), fc.stringMatching(/^[ \t]{0,4}$/)),
          { maxLength: 12 },
        ),
        (lines) => {
          const plain = lines.map(([text]) => text).join('\n');
          const padded = lines.map(([text, padding]) => text + padding).join('\n');
          expect(parseNotes(padded)).toEqual(parseNotes(plain));
        },
      ),
    );
  });

  it('carries markup through as the plain text it is, whatever comes before it', () => {
    fc.assert(
      fc.property(
        markdownish,
        fc.constantFrom('<script>alert(1)</script>', '<img src=x onerror=alert(1)>', '&lt;b&gt;'),
        (markdown, markup) => {
          const blocks = parseNotes(`${markdown}\n\n${markup}`);
          // A string in a text run, which React escapes, never a node or an attribute of its own.
          expect(blocks[blocks.length - 1]).toEqual({ kind: 'paragraph', inlines: [{ kind: 'text', text: markup }] });
        },
      ),
    );
  });
});

describe('parseInline', () => {
  it('invents nothing: the visible text can be read out of the input in order', () => {
    fc.assert(
      fc.property(anyText.map((text) => text.replace(/[\r\n]/g, ' ')), (line) => {
        const visible = parseInline(line)
          .map((inline) => inline.text)
          .join('');
        expect(isSubsequence(visible, line)).toBe(true);
      }),
    );
  });

  it('returns text free of markup characters unchanged, as a single run', () => {
    fc.assert(
      fc.property(
        fc.stringMatching(/^[A-Za-z0-9 .,;:!?'"<>&=/-]{1,60}$/).filter((text) => !/https?:\/\//.test(text)),
        (line) => {
          expect(parseInline(line)).toEqual([{ kind: 'text', text: line }]);
        },
      ),
    );
  });

  it('keeps the whole line when every marker in it is unmatched', () => {
    fc.assert(
      fc.property(
        fc.stringMatching(/^[a-z ]{0,20}$/),
        fc.constantFrom('**', '`', '['),
        fc.stringMatching(/^[a-z ]{0,20}$/),
        (before, marker, after) => {
          const line = `${before}${marker}${after}`;
          expect(
            parseInline(line)
              .map((inline) => inline.text)
              .join(''),
          ).toBe(line);
        },
      ),
    );
  });
});
