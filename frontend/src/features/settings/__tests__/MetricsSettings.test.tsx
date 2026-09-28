// @vitest-environment jsdom

/**
 * The Monitoring card: where Prometheus reads the metrics, a scrape config to paste, and the scrape token a
 * scraper needs while sign in is on. The token is shown once, when it is made, and never again, so the card
 * must show it then, offer to copy it, and forget it when the admin moves on. Rotating and revoking break a
 * running scraper, so both ask first. The store's requests are replaced with spies.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { formatDateTime } from '@/lib/format';
import { TOKEN_FILE } from '@/lib/scrapeConfig';
import { useAuthStore } from '@/store/useAuthStore';
import { useMetricsStore } from '@/store/useMetricsStore';

import { MetricsSettings } from '../MetricsSettings';
import { account } from './fixtures';

let refresh: Mock<() => Promise<void>>;
let create: Mock<() => Promise<void>>;
let revoke: Mock<() => Promise<void>>;
let forgetFreshToken: Mock<() => void>;

const TOKEN = '9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08';
const MADE = 1_790_000_000_000;

function renderCard() {
  const user = userEvent.setup();
  const view = render(<MetricsSettings />);
  return { user, ...view };
}

const snippet = () => screen.getByLabelText('Prometheus scrape config');

describe('the monitoring card', () => {
  beforeEach(() => {
    refresh = vi.fn(async () => undefined);
    create = vi.fn(async () => undefined);
    revoke = vi.fn(async () => undefined);
    forgetFreshToken = vi.fn();
    useAuthStore.setState({ mode: 'accounts', user: account(1, 'admin') });
    useMetricsStore.setState({
      createdAtMs: null,
      loaded: true,
      freshToken: null,
      busy: false,
      error: null,
      refresh,
      create,
      revoke,
      forgetFreshToken,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  describe('where the metrics are', () => {
    it('asks whether a token exists when shown', () => {
      renderCard();
      expect(refresh).toHaveBeenCalledTimes(1);
    });

    it('names the metrics address on this recorder', () => {
      renderCard();
      expect(screen.getByText(`${window.location.origin}/api/metrics`)).toBeInTheDocument();
    });

    it('offers a scrape config for the path under the API, on the address this page was opened on', () => {
      renderCard();
      const host = new URL(window.location.origin);
      expect(snippet()).toHaveTextContent('metrics_path: /api/metrics');
      expect(snippet().textContent).toContain(`targets: ['${host.hostname}:`);
    });
  });

  describe('with no token', () => {
    it('says a scraper needs one while sign in is on', () => {
      renderCard();
      expect(screen.getByText(/No scrape token\./)).toBeInTheDocument();
      expect(screen.getByText(/needs one to read the metrics while sign in is on/)).toBeInTheDocument();
      expect(snippet()).toHaveTextContent(`credentials_file: ${TOKEN_FILE}`);
    });

    it('says an open recorder needs none, and leaves the credentials out of the config', () => {
      useAuthStore.setState({ mode: 'open', user: null });
      renderCard();
      expect(screen.getByText(/not needed while anyone on the network can use the recorder/)).toBeInTheDocument();
      expect(snippet().textContent).not.toContain('authorization');
    });

    it('makes one when asked, without a confirmation, since nothing can break', async () => {
      const { user } = renderCard();
      await user.click(screen.getByRole('button', { name: 'Create token' }));
      expect(create).toHaveBeenCalledTimes(1);
    });

    it('waits while a request is out', () => {
      useMetricsStore.setState({ busy: true });
      renderCard();
      expect(screen.getByRole('button', { name: 'Create token' })).toBeDisabled();
    });

    it('holds its answer until the service replies', () => {
      useMetricsStore.setState({ loaded: false });
      renderCard();
      expect(screen.queryByRole('button', { name: 'Create token' })).not.toBeInTheDocument();
      expect(screen.queryByText(/No scrape token/)).not.toBeInTheDocument();
    });
  });

  describe('a token just made', () => {
    beforeEach(() => {
      useMetricsStore.setState({ createdAtMs: MADE, freshToken: TOKEN });
    });

    it('is shown once, with where to put it', () => {
      renderCard();
      expect(screen.getByLabelText('New scrape token')).toHaveTextContent(TOKEN);
      expect(screen.getByText(/will not be shown again/)).toBeInTheDocument();
      expect(screen.getAllByText(new RegExp(TOKEN_FILE)).length).toBeGreaterThan(0);
    });

    it('is forgotten when the card is left', () => {
      const { unmount } = renderCard();
      expect(forgetFreshToken).not.toHaveBeenCalled();
      unmount();
      expect(forgetFreshToken).toHaveBeenCalledTimes(1);
    });

    it('can be copied where the browser allows the clipboard', async () => {
      const writeText = vi.fn(async () => undefined);
      vi.stubGlobal('isSecureContext', true);
      const { user } = renderCard();
      // userEvent installs its own clipboard on setup, so the stand in goes on after it.
      Object.defineProperty(window.navigator, 'clipboard', { value: { writeText }, configurable: true });
      await user.click(screen.getByRole('button', { name: 'Copy the token' }));
      expect(writeText).toHaveBeenCalledWith(TOKEN);
    });

    it('offers no copy button on a plain http address, where the clipboard is refused', () => {
      vi.stubGlobal('isSecureContext', false);
      renderCard();
      expect(screen.queryByRole('button', { name: 'Copy the token' })).not.toBeInTheDocument();
      expect(screen.getByLabelText('New scrape token')).toHaveTextContent(TOKEN);
    });
  });

  describe('an existing token', () => {
    beforeEach(() => {
      useMetricsStore.setState({ createdAtMs: MADE, freshToken: null });
    });

    it('shows when it was made, and never the token', () => {
      renderCard();
      expect(screen.getByText(`Scrape token created ${formatDateTime(MADE)}.`)).toBeInTheDocument();
      expect(screen.queryByLabelText('New scrape token')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Create token' })).not.toBeInTheDocument();
    });

    it('puts the credentials in the config even on an open recorder, since the scraper will send them', () => {
      useAuthStore.setState({ mode: 'open', user: null });
      renderCard();
      expect(snippet()).toHaveTextContent('type: Bearer');
    });

    it('asks before rotating, since the old token stops working at once', async () => {
      const { user } = renderCard();
      await user.click(screen.getByRole('button', { name: 'Rotate' }));
      expect(create).not.toHaveBeenCalled();
      expect(screen.getByText(/stops working at once/)).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'Make a new token' }));
      expect(create).toHaveBeenCalledTimes(1);
    });

    it('keeps the token when rotating is called off', async () => {
      const { user } = renderCard();
      await user.click(screen.getByRole('button', { name: 'Rotate' }));
      await user.click(screen.getByRole('button', { name: 'Keep' }));
      expect(create).not.toHaveBeenCalled();
      expect(screen.getByRole('button', { name: 'Rotate' })).toBeInTheDocument();
    });

    it('asks before revoking', async () => {
      const { user } = renderCard();
      await user.click(screen.getByRole('button', { name: 'Revoke' }));
      expect(revoke).not.toHaveBeenCalled();
      await user.click(screen.getByRole('button', { name: 'Revoke the token' }));
      expect(revoke).toHaveBeenCalledTimes(1);
    });

    it('keeps the token when revoking is called off', async () => {
      const { user } = renderCard();
      await user.click(screen.getByRole('button', { name: 'Revoke' }));
      await user.click(screen.getByRole('button', { name: 'Keep' }));
      expect(revoke).not.toHaveBeenCalled();
    });
  });

  it('shows what went wrong', () => {
    useMetricsStore.setState({ error: 'database is locked' });
    renderCard();
    expect(screen.getByText('database is locked')).toBeInTheDocument();
  });
});
