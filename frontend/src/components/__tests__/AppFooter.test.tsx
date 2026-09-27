// @vitest-environment jsdom

/**
 * The strip at the bottom of every page: attribution, licence, the repository, where to report a fault,
 * and the version to quote when doing so. The status store's loadVersion is a spy, so these test that the
 * version is asked for once and shown only when known.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { useStatusStore } from '@/store/useStatusStore';

import { AppFooter } from '../AppFooter';

let loadVersion: Mock<() => Promise<void>>;

describe('the footer', () => {
  beforeEach(() => {
    loadVersion = vi.fn(async () => undefined);
    useStatusStore.setState({ version: null, loadVersion });
  });

  it('is the page footer', () => {
    render(<AppFooter />);
    expect(screen.getByRole('contentinfo')).toBeInTheDocument();
  });

  it('asks for the version once when it appears', () => {
    const { rerender } = render(<AppFooter />);
    rerender(<AppFooter />);
    expect(loadVersion).toHaveBeenCalledTimes(1);
  });

  it('shows no version until it is known', () => {
    render(<AppFooter />);
    expect(screen.queryByTitle('The version to quote in a bug report')).not.toBeInTheDocument();
  });

  it('shows the version with a v, and says it is the one to quote in a bug report', () => {
    useStatusStore.setState({ version: '0.4.0-beta.2' });
    render(<AppFooter />);
    expect(screen.getByTitle('The version to quote in a bug report')).toHaveTextContent('v0.4.0-beta.2');
  });

  it('shows the version as soon as it arrives', () => {
    render(<AppFooter />);
    act(() => {
      useStatusStore.setState({ version: '1.0.0' });
    });
    expect(screen.getByText('v1.0.0')).toBeInTheDocument();
  });

  it('credits the author this year, linking to the portfolio in a new tab', () => {
    render(<AppFooter />);
    const footer = screen.getByRole('contentinfo');
    expect(footer).toHaveTextContent(`\u00a9 ${new Date().getFullYear()} MD. Shibbir Ahmed`);
    const link = screen.getByRole('link', { name: 'MD. Shibbir Ahmed' });
    expect(link).toHaveAttribute('href', 'https://shibbirweb.github.io');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer noopener');
  });

  it('names the licence', () => {
    render(<AppFooter />);
    expect(screen.getByText('MIT licence')).toBeInTheDocument();
  });

  it('links to the repository in a new tab', () => {
    render(<AppFooter />);
    const link = screen.getByRole('link', { name: 'Star the repository' });
    expect(link).toHaveAttribute('href', 'https://github.com/shibbirweb/on-air-record');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer noopener');
  });

  it('links to the issue tracker in a new tab', () => {
    render(<AppFooter />);
    const link = screen.getByRole('link', { name: 'Something wrong? Report an issue' });
    expect(link).toHaveAttribute('href', 'https://github.com/shibbirweb/on-air-record/issues');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer noopener');
  });
});
