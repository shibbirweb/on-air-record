// @vitest-environment jsdom

/**
 * Release notes drawn from a GitHub release body. The body is written by whoever publishes a release, so
 * it is treated as data: the handful of Markdown constructs the changelog uses become real headings,
 * lists, emphasis, code and links, and anything else, HTML above all, is shown as the text it is. The
 * parser itself is tested in lib; these prove what reaches the page.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { ReleaseNotes } from '../ReleaseNotes';

describe('release notes', () => {
  it('say so when a release has no notes', () => {
    render(<ReleaseNotes markdown={'  \n\n'} />);
    expect(screen.getByText('No notes were written for this release.')).toBeInTheDocument();
  });

  it('draw headings, bullet lists and paragraphs as such', () => {
    render(
      <ReleaseNotes markdown={'Thanks for trying the beta.\n\n### Fixed\n\n- Playback resumes [OAR-9]\n- The meter redraws'} />,
    );
    expect(screen.getByRole('heading', { name: 'Fixed' })).toBeInTheDocument();
    expect(screen.getByText('Thanks for trying the beta.').closest('p')).not.toBeNull();
    const items = screen.getAllByRole('listitem');
    expect(items.map((item) => item.textContent)).toEqual(['Playback resumes [OAR-9]', 'The meter redraws']);
  });

  it('drop the bold markers a heading was written with', () => {
    render(<ReleaseNotes markdown={'## **Breaking**'} />);
    expect(screen.getByRole('heading', { name: 'Breaking' })).toBeInTheDocument();
  });

  it('join a wrapped bullet back into one item', () => {
    render(<ReleaseNotes markdown={'- A long entry that\n  carries on here'} />);
    expect(screen.getByRole('listitem')).toHaveTextContent('A long entry that carries on here');
  });

  it('show bold and inline code', () => {
    render(<ReleaseNotes markdown={'Run **once** with `--update`.'} />);
    expect(screen.getByText('once').tagName).toBe('STRONG');
    expect(screen.getByText('--update').tagName).toBe('CODE');
  });

  it('open web links in a new tab without handing it this page', () => {
    render(<ReleaseNotes markdown={'See [the guide](https://example.com/guide).'} />);
    const link = screen.getByRole('link', { name: 'the guide' });
    expect(link).toHaveAttribute('href', 'https://example.com/guide');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer noopener');
  });

  it('link a bare address, leaving off the full stop after it', () => {
    render(<ReleaseNotes markdown={'Details at https://example.com/notes.'} />);
    expect(screen.getByRole('link', { name: 'https://example.com/notes' })).toHaveAttribute(
      'href',
      'https://example.com/notes',
    );
  });

  it('show a link to anything but the web as its words, not a link', () => {
    render(<ReleaseNotes markdown={'[Click me](javascript:alert(1)) now'} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.getByText('Click me')).toBeInTheDocument();
  });

  it('show HTML in a release body as text, never as markup', () => {
    const { container } = render(
      <ReleaseNotes
        markdown={'<img src="x" onerror="alert(1)"> and <script>alert(2)</script>\n\n- <b>bold?</b>'}
      />,
    );
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('b')).toBeNull();
    expect(screen.getByText(/<img src="x" onerror="alert\(1\)"> and <script>alert\(2\)<\/script>/)).toBeInTheDocument();
    expect(screen.getByRole('listitem')).toHaveTextContent('<b>bold?</b>');
  });
});
