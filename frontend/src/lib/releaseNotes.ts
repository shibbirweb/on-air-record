/**
 * Release notes, from the Markdown written on a GitHub release to something React can draw.
 *
 * Only the handful of constructs the changelog uses are understood: headings, bullet lists with wrapped
 * lines, paragraphs, bold, inline code, links, and bare web addresses, which GitHub's generated notes are
 * full of. Everything else is shown as the plain text it is. The
 * result is data, never HTML, so nothing in a release body can inject markup, and links are kept only
 * when they point at http or https.
 */

export type Inline =
  { kind: 'text' | 'bold' | 'code'; text: string } | { kind: 'link'; text: string; href: string };

export type NoteBlock =
  | { kind: 'heading'; text: string }
  | { kind: 'paragraph'; inlines: Inline[] }
  | { kind: 'list'; items: Inline[][] };

// A bare address stops before trailing punctuation, so "see https://example.com." links without the dot.
const INLINE = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)|(https?:\/\/[^\s<>()]*[^\s<>().,;:!?'"])/g;

/** Split one line of text into plain runs, bold, code and links. */
export function parseInline(text: string): Inline[] {
  const parts: Inline[] = [];
  let last = 0;
  for (const match of text.matchAll(INLINE)) {
    const start = match.index ?? 0;
    if (start > last) {
      parts.push({ kind: 'text', text: text.slice(last, start) });
    }
    const [whole, bold, code, linkText, href, bare] = match;
    if (bare !== undefined) {
      parts.push({ kind: 'link', text: bare, href: bare });
    } else if (bold !== undefined) {
      parts.push({ kind: 'bold', text: bold });
    } else if (code !== undefined) {
      parts.push({ kind: 'code', text: code });
    } else if (linkText !== undefined && href !== undefined && /^https?:\/\//i.test(href)) {
      parts.push({ kind: 'link', text: linkText, href });
    } else {
      // A link to anything but the web is shown as the words it wraps, not followed.
      parts.push({ kind: 'text', text: linkText ?? whole });
    }
    last = start + whole.length;
  }
  if (last < text.length) {
    parts.push({ kind: 'text', text: text.slice(last) });
  }
  return parts;
}

/** Read a release body into headings, paragraphs and lists. */
export function parseNotes(markdown: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  let paragraph: string[] = [];
  let list: string[] | null = null;

  const flush = () => {
    if (paragraph.length > 0) {
      blocks.push({ kind: 'paragraph', inlines: parseInline(paragraph.join(' ')) });
      paragraph = [];
    }
    if (list) {
      blocks.push({ kind: 'list', items: list.map(parseInline) });
      list = null;
    }
  };

  for (const raw of markdown.replace(/\r\n?/g, '\n').split('\n')) {
    const line = raw.trimEnd();
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    const bullet = /^[-*]\s+(.*)$/.exec(line);

    if (line.trim() === '') {
      flush();
    } else if (heading) {
      flush();
      blocks.push({ kind: 'heading', text: heading[1].replace(/\*\*/g, '').trim() });
    } else if (bullet) {
      if (paragraph.length > 0) {
        flush();
      }
      list = list ?? [];
      list.push(bullet[1].trim());
    } else if (list && /^\s+\S/.test(line)) {
      // An indented line continues the bullet above it, which is how the changelog wraps long entries.
      list[list.length - 1] += ` ${line.trim()}`;
    } else {
      if (list) {
        flush();
      }
      paragraph.push(line.trim());
    }
  }
  flush();
  return blocks;
}
