// @vitest-environment jsdom

/**
 * The chrome around every page: the page links, the on air light, the connection badge, the theme switch,
 * the account menu, the update notice and the footer, and the router outlet that hands pages the audio
 * engine. Which of these a visitor sees depends on the access mode and role, which is the part most worth
 * pinning. The stream engine hook is replaced, because a real one builds a Web Audio graph and opens a
 * socket, neither of which jsdom has; the stores' fetching actions are spies.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { AuthMode, CaptureState, ServiceStatus, UpdateStatus, User } from '@/api/types';

const stream = vi.hoisted(() => ({
  engine: { name: 'the page engine' },
  playheadMs: () => 1_757_034_000_000,
  /** How often the engine was set up and torn down, which would stop the broadcast and drop the socket. */
  mounts: 0,
  unmounts: 0,
}));

vi.mock('@/hooks/useStreamEngine', async () => {
  const { useEffect } = await import('react');
  return {
    useStreamEngine: () => {
      useEffect(() => {
        stream.mounts += 1;
        return () => {
          stream.unmounts += 1;
        };
      }, []);
      return stream;
    },
  };
});

const { AppShell, useAppContext } = await import('../AppShell');
const { useAuthStore } = await import('@/store/useAuthStore');
const { useConnectionStore } = await import('@/store/useConnectionStore');
const { useStatusStore } = await import('@/store/useStatusStore');
const { useTransportStore } = await import('@/store/useTransportStore');
const { useUpdateStore } = await import('@/store/useUpdateStore');

const LIT = 'bg-[var(--live)]';

function account(role: User['role']): User {
  return { id: 1, email: `${role}@example.com`, role, createdAtMs: 0, twoFactorEnabled: false };
}

function status(state: CaptureState): ServiceStatus {
  return {
    capture: {
      state,
      sessionId: 1,
      deviceId: null,
      deviceName: null,
      sampleRate: 48_000,
      channels: 1,
      frameMs: 100,
      startedAtMs: null,
      droppedFrames: 0,
      error: null,
    },
    levels: { rms: 0, peak: 0 },
    listeners: 1,
    serverTimeMs: 0,
    liveEdgeMs: null,
  };
}

const UPDATE: UpdateStatus = {
  currentVersion: '0.4.0',
  channel: 'stable',
  automatic: true,
  checkedAtMs: 0,
  error: null,
  available: {
    version: '0.5.0',
    tag: 'v0.5.0',
    prerelease: false,
    publishedAtMs: 0,
    notes: '',
    url: 'https://github.com/shibbirweb/on-air-record/releases/tag/v0.5.0',
  },
  releases: [],
  install: { kind: 'manual', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' },
  releasesUrl: 'https://github.com/shibbirweb/on-air-record/releases',
};

/** A page that shows what the shell handed it through the outlet. */
function ContextProbe() {
  const { engine, playheadMs } = useAppContext();
  return (
    <p>
      Control room page with {(engine as unknown as { name: string }).name} at {playheadMs()}
    </p>
  );
}

function renderShell(path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route element={<AppShell />}>
          <Route index element={<ContextProbe />} />
          <Route path="settings" element={<p>Settings page</p>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

function signIn(mode: AuthMode, user: User | null) {
  useAuthStore.setState({ mode, user, loaded: true });
}

const settingsLink = () => screen.queryByRole('link', { name: 'Settings' });
const controlRoomLink = () => screen.getByRole('link', { name: 'Control room' });
const onAir = () => screen.getByText('On air');

let refreshStatus: Mock<() => Promise<void>>;
let refreshUpdates: Mock<() => Promise<void>>;

describe('the app shell', () => {
  beforeEach(() => {
    window.localStorage.clear();
    refreshStatus = vi.fn(async () => undefined);
    refreshUpdates = vi.fn(async () => undefined);
    signIn('open', null);
    useStatusStore.setState({
      status: null,
      version: null,
      refresh: refreshStatus,
      loadVersion: vi.fn(async () => undefined),
    });
    useConnectionStore.setState({ connected: true });
    useTransportStore.setState({ playing: false, mode: 'live' });
    useUpdateStore.setState({ status: null, dismissed: null, refresh: refreshUpdates });
  });

  it('names the app in its header', () => {
    renderShell();
    expect(screen.getByRole('heading', { level: 1, name: 'On Air Record' })).toBeInTheDocument();
    expect(screen.getByText('Local network audio broadcast and DVR')).toBeInTheDocument();
  });

  it('hands the page the audio engine and the playhead through the outlet', () => {
    renderShell();
    expect(screen.getByText('Control room page with the page engine at 1757034000000')).toBeInTheDocument();
  });

  it('ends every page with the footer', () => {
    renderShell();
    expect(screen.getByRole('contentinfo')).toHaveTextContent('Report an issue');
  });

  it('starts polling the recorder status straight away', () => {
    renderShell();
    expect(refreshStatus).toHaveBeenCalledTimes(1);
  });

  describe('the page links', () => {
    it('offers both pages on an open recorder, marking the current one', () => {
      renderShell('/');
      expect(controlRoomLink()).toHaveAttribute('aria-current', 'page');
      expect(settingsLink()).toBeInTheDocument();
      expect(settingsLink()).not.toHaveAttribute('aria-current');
    });

    it('offers both pages to an admin', () => {
      signIn('accounts', account('admin'));
      renderShell();
      expect(settingsLink()).toBeInTheDocument();
    });

    it('offers both pages while the first run question is still open', () => {
      signIn('undecided', null);
      renderShell();
      expect(settingsLink()).toBeInTheDocument();
    });

    it('does not offer the settings page to a listener', () => {
      signIn('accounts', account('listener'));
      renderShell();
      expect(controlRoomLink()).toBeInTheDocument();
      expect(settingsLink()).not.toBeInTheDocument();
    });

    it('moves between pages, and the current mark follows', async () => {
      const user = userEvent.setup();
      renderShell('/');
      await user.click(settingsLink() as HTMLElement);

      expect(screen.getByText('Settings page')).toBeInTheDocument();
      expect(settingsLink()).toHaveAttribute('aria-current', 'page');
      expect(controlRoomLink()).not.toHaveAttribute('aria-current');

      await user.click(controlRoomLink());
      expect(screen.getByText(/Control room page/)).toBeInTheDocument();
    });

    it('keeps the one audio engine through every move between pages, so playback never stops', async () => {
      const user = userEvent.setup();
      stream.mounts = 0;
      stream.unmounts = 0;
      renderShell('/');
      await user.click(settingsLink() as HTMLElement);
      await user.click(controlRoomLink());
      await user.click(settingsLink() as HTMLElement);

      expect(stream.mounts).toBe(1);
      expect(stream.unmounts).toBe(0);
    });
  });

  describe('the on air light', () => {
    it('is lit when recording, playing and on the live feed', () => {
      useStatusStore.setState({ status: status('recording') });
      useTransportStore.setState({ playing: true, mode: 'live' });
      renderShell();
      expect(onAir()).toHaveClass(LIT);
    });

    it('is dark when nobody pressed play', () => {
      useStatusStore.setState({ status: status('recording') });
      useTransportStore.setState({ playing: false, mode: 'live' });
      renderShell();
      expect(onAir()).not.toHaveClass(LIT);
    });

    it('is dark when listening to the past', () => {
      useStatusStore.setState({ status: status('recording') });
      useTransportStore.setState({ playing: true, mode: 'playback' });
      renderShell();
      expect(onAir()).not.toHaveClass(LIT);
    });

    it('is dark when the recorder is not recording', () => {
      useStatusStore.setState({ status: status('idle') });
      useTransportStore.setState({ playing: true, mode: 'live' });
      renderShell();
      expect(onAir()).not.toHaveClass(LIT);
    });

    it('is dark before the status is known', () => {
      useTransportStore.setState({ playing: true, mode: 'live' });
      renderShell();
      expect(onAir()).not.toHaveClass(LIT);
    });

    it('lights up the moment play is pressed', () => {
      useStatusStore.setState({ status: status('recording') });
      renderShell();
      expect(onAir()).not.toHaveClass(LIT);
      act(() => {
        useTransportStore.setState({ playing: true });
      });
      expect(onAir()).toHaveClass(LIT);
    });
  });

  describe('the connection badge', () => {
    it('says the stream is connected', () => {
      renderShell();
      expect(screen.getByText('Stream connected')).toBeInTheDocument();
    });

    it('says the stream is offline when the socket drops', () => {
      renderShell();
      act(() => {
        useConnectionStore.setState({ connected: false });
      });
      expect(screen.getByText('Stream offline')).toBeInTheDocument();
      expect(screen.queryByText('Stream connected')).not.toBeInTheDocument();
    });
  });

  it('shows how many are listening', () => {
    useStatusStore.setState({ status: status('recording') });
    renderShell();
    expect(screen.getByText('1 listener')).toBeInTheDocument();
  });

  describe('the theme switch', () => {
    it('switches between dark and light and remembers the choice', async () => {
      const user = userEvent.setup();
      renderShell();
      const toggle = screen.getByRole('button', { name: 'Toggle colour theme' });
      expect(document.documentElement).toHaveClass('dark');

      await user.click(toggle);
      expect(document.documentElement).not.toHaveClass('dark');
      expect(window.localStorage.getItem('oar-theme')).toBe('light');

      await user.click(toggle);
      expect(document.documentElement).toHaveClass('dark');
      expect(window.localStorage.getItem('oar-theme')).toBe('dark');
    });

    it('starts from the theme chosen before', () => {
      window.localStorage.setItem('oar-theme', 'light');
      renderShell();
      expect(document.documentElement).not.toHaveClass('dark');
    });
  });

  describe('the account menu', () => {
    it('is there for somebody signed in', () => {
      signIn('accounts', account('listener'));
      renderShell();
      expect(screen.getByRole('button', { name: 'Account: listener@example.com' })).toBeInTheDocument();
    });

    it('is not there on an open recorder', () => {
      renderShell();
      expect(screen.queryByRole('button', { name: /^Account:/ })).not.toBeInTheDocument();
    });
  });

  describe('the update notice', () => {
    it('is shown to an admin, whose update check runs', () => {
      signIn('accounts', account('admin'));
      useUpdateStore.setState({ status: UPDATE });
      renderShell();
      expect(screen.getByText('On Air Record 0.5.0 is available.')).toBeInTheDocument();
      expect(refreshUpdates).toHaveBeenCalledTimes(1);
    });

    it('is shown on an open recorder, where everyone may administer', () => {
      useUpdateStore.setState({ status: UPDATE });
      renderShell();
      expect(screen.getByText('On Air Record 0.5.0 is available.')).toBeInTheDocument();
      expect(refreshUpdates).toHaveBeenCalledTimes(1);
    });

    it('is hidden from a listener, whose browser does not even ask', () => {
      signIn('accounts', account('listener'));
      useUpdateStore.setState({ status: UPDATE });
      renderShell();
      expect(screen.queryByText('On Air Record 0.5.0 is available.')).not.toBeInTheDocument();
      expect(refreshUpdates).not.toHaveBeenCalled();
    });
  });
});
