// @vitest-environment jsdom

/**
 * The strip under the header announcing a newer release. It appears only when the service found one, opens
 * the what's new dialog, and "Later" hides it for that version in this browser until the next release
 * comes out. The real update store is used, so dismissing is remembered where the store keeps it. Whether
 * listeners see the strip at all is decided by the page that renders it, not by the strip.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { useUpdateStore } from '@/store/useUpdateStore';

import { UpdateBanner } from '../UpdateBanner';
import { release, updateStatus, upToDate } from './fixtures';

const laterButton = (version: string) =>
  screen.getByRole('button', { name: `Hide until the release after ${version}` });

describe('the update banner', () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUpdateStore.setState({ status: updateStatus(), dismissed: null, checking: false, requestError: null });
  });

  afterEach(() => {
    window.localStorage.clear();
  });

  it('says which release is out and which one is running', () => {
    render(<UpdateBanner />);
    expect(screen.getByText('On Air Record 0.9.0 is available.')).toBeInTheDocument();
    expect(screen.getByText('You have 0.8.1.')).toBeInTheDocument();
  });

  it('stays away before the service has answered', () => {
    useUpdateStore.setState({ status: null });
    const { container } = render(<UpdateBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it('stays away when this is the newest release', () => {
    useUpdateStore.setState({ status: upToDate() });
    const { container } = render(<UpdateBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it('opens what is new and how to update', async () => {
    const user = userEvent.setup();
    render(<UpdateBanner />);
    await user.click(screen.getByRole('button', { name: "What's new and how to update" }));
    expect(screen.getByRole('dialog', { name: "What's new in 0.9.0" })).toBeInTheDocument();
  });

  it('hides for this release when put off until later, and remembers it in this browser', async () => {
    const user = userEvent.setup();
    const { container } = render(<UpdateBanner />);
    await user.click(laterButton('0.9.0'));

    expect(container).toBeEmptyDOMElement();
    expect(useUpdateStore.getState().dismissed).toBe('0.9.0');
    expect(window.localStorage.getItem('oar.updates.dismissed')).toBe('0.9.0');
  });

  it('stays hidden for a release already put off', () => {
    useUpdateStore.setState({ dismissed: '0.9.0' });
    const { container } = render(<UpdateBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it('comes back when the release after the one put off appears', () => {
    const next = release('0.9.1');
    useUpdateStore.setState({ dismissed: '0.9.0', status: updateStatus({ available: next, releases: [next] }) });
    render(<UpdateBanner />);
    expect(screen.getByText('On Air Record 0.9.1 is available.')).toBeInTheDocument();
    expect(laterButton('0.9.1')).toBeInTheDocument();
  });
});
