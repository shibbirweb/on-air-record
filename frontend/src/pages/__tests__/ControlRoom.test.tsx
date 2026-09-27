// @vitest-environment jsdom

/**
 * The control room page: that it lays out the broadcast, the timeline with its toolbar and day overview,
 * the recorder, the source and the storage panels; that the broadcast card says what is playing; that the
 * playhead from the shell reaches the controls; and that it keeps its data fresh, polling each source at
 * its own pace, refetching the waveform once a pan or zoom settles and the day overview when the day
 * changes. Each panel has its own tests; this is the page's wiring. Every fetch is a spy, so nothing here
 * touches the network.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { AppOutletContext } from '@/components/AppShell';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AudioEngine } from '@/lib/audio/audioEngine';
import { formatClock } from '@/lib/format';
import { useAuthStore } from '@/store/useAuthStore';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useDeviceStore } from '@/store/useDeviceStore';
import { useListenersStore } from '@/store/useListenersStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useStorageStore } from '@/store/useStorageStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';
import { recordCanvases } from '@/test/canvas';
import type { CanvasRecorder } from '@/test/canvas';

import { ControlRoom } from '../ControlRoom';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();
const SPAN = 15 * 60_000;

const pristine = {
  auth: useAuthStore.getState(),
  bookmarks: useBookmarkStore.getState(),
  connection: useConnectionStore.getState(),
  devices: useDeviceStore.getState(),
  listeners: useListenersStore.getState(),
  status: useStatusStore.getState(),
  storage: useStorageStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

type Refresh = Mock<() => Promise<void>>;

let refreshRange: Refresh;
let refreshPeaks: Refresh;
let refreshDays: Refresh;
let refreshDayPeaks: Refresh;
let refreshStorage: Refresh;
let refreshBookmarks: Refresh;
let refreshDevices: Refresh;
let canvas: CanvasRecorder;

/** The page inside a stand in for the shell, which hands it the engine and the playhead. */
function renderPage(playheadMs: () => number | null = () => null) {
  const context: AppOutletContext = {
    engine: { analyser: null } as unknown as AudioEngine,
    playheadMs,
  };
  return render(
    <TooltipProvider>
      <MemoryRouter>
        <Routes>
          <Route element={<Outlet context={context} />}>
            <Route index element={<ControlRoom />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </TooltipProvider>,
  );
}

function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

beforeEach(() => {
  for (const [store, state] of [
    [useAuthStore, pristine.auth],
    [useBookmarkStore, pristine.bookmarks],
    [useConnectionStore, pristine.connection],
    [useDeviceStore, pristine.devices],
    [useListenersStore, pristine.listeners],
    [useStatusStore, pristine.status],
    [useStorageStore, pristine.storage],
    [useTimelineStore, pristine.timeline],
    [useTransportStore, pristine.transport],
  ] as const) {
    (store as { setState: (value: unknown, replace: true) => void }).setState(state, true);
  }

  // Only the timers the page polls and debounces with; the animation frames are cranked by the recorder.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] });
  canvas = recordCanvases(800);

  const refresh = () => vi.fn(async () => undefined);
  refreshRange = refresh();
  refreshPeaks = refresh();
  refreshDays = refresh();
  refreshDayPeaks = refresh();
  refreshStorage = refresh();
  refreshBookmarks = refresh();
  refreshDevices = refresh();

  useAuthStore.setState({ mode: 'open', user: null });
  useConnectionStore.setState({ connected: true });
  useTimelineStore.setState({
    windowStartMs: NOW - SPAN,
    spanMs: SPAN,
    followingLive: true,
    range: { earliestMs: NOW - 86_400_000, latestMs: NOW, liveEdgeMs: NOW, serverTimeMs: NOW, coverage: [] },
    refreshRange,
    refreshPeaks,
    refreshDays,
    refreshDayPeaks,
  });
  useStorageStore.setState({ refresh: refreshStorage });
  useBookmarkStore.setState({ refresh: refreshBookmarks });
  useDeviceStore.setState({ refresh: refreshDevices });
});

afterEach(() => {
  canvas.restore();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('the layout', () => {
  it('has the broadcast, timeline, recorder, source and storage panels', () => {
    renderPage();
    for (const title of ['Broadcast', 'Timeline', 'Recorder', 'Source', 'Storage']) {
      expect(screen.getByText(title, { selector: '[data-slot="card-title"]' })).toBeInTheDocument();
    }
  });

  it('puts the transport under the broadcast, and the toolbar, timeline and day overview in the timeline', () => {
    renderPage();
    expect(screen.getByRole('button', { name: 'Play' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Zoom in' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Timeline' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Day overview' })).toBeInTheDocument();
  });

  it('puts the recorder state, the microphone chooser and the disk figures in the side rail', () => {
    renderPage();
    expect(screen.getByText('Idle')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Input source' })).toBeInTheDocument();
    expect(screen.getByText('On disk')).toBeInTheDocument();
  });

  it('explains how to use the timeline', () => {
    renderPage();
    expect(screen.getByText(/Click anywhere to play from that moment, drag to pan, scroll to zoom/)).toBeInTheDocument();
  });
});

describe('the broadcast description', () => {
  it('asks for a click before anything plays, since browsers require one', () => {
    renderPage();
    expect(
      screen.getByText('Press play to start listening. Browsers only allow audio after a click.'),
    ).toBeInTheDocument();
  });

  it('says the live feed is playing', () => {
    useTransportStore.setState({ playing: true, mode: 'live' });
    renderPage(() => NOW);
    expect(screen.getByText('Playing the live feed from the studio microphone.')).toBeInTheDocument();
  });

  it('says recorded audio is playing', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderPage(() => NOW - 600_000);
    expect(screen.getByText('Playing back recorded audio from the timeline.')).toBeInTheDocument();
  });
});

describe('the playhead from the shell', () => {
  it('reaches the transport clock', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderPage(() => NOW - 254_000);
    act(() => {
      canvas.frame();
    });
    expect(screen.getByText(formatClock(NOW - 254_000))).toBeInTheDocument();
  });
});

describe('for a listener', () => {
  it('shows the recorder and the microphone without the controls to change them', () => {
    useAuthStore.setState({
      mode: 'accounts',
      user: { id: 2, email: 'kitchen@example.com', role: 'listener', createdAtMs: NOW, twoFactorEnabled: false },
    });
    renderPage();
    expect(screen.queryByRole('button', { name: 'Record' })).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Input source' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Add a bookmark here' })).not.toBeInTheDocument();
  });
});

describe('keeping the data fresh', () => {
  it('fetches everything as soon as it opens', () => {
    renderPage();
    for (const refresh of [refreshRange, refreshStorage, refreshDays, refreshDayPeaks, refreshBookmarks, refreshDevices]) {
      expect(refresh).toHaveBeenCalled();
    }
  });

  it('polls the timeline every two seconds and the rest at their own slower pace', () => {
    renderPage();
    const counts = () => ({
      range: refreshRange.mock.calls.length,
      storage: refreshStorage.mock.calls.length,
      days: refreshDays.mock.calls.length,
      bookmarks: refreshBookmarks.mock.calls.length,
    });
    const before = counts();

    advance(2_000);
    expect(counts()).toEqual({ ...before, range: before.range + 1 });

    advance(8_000);
    expect(counts().range).toBe(before.range + 5);
    expect(counts().storage).toBe(before.storage + 1);
    expect(counts().bookmarks).toBe(before.bookmarks);

    advance(10_000);
    expect(counts().bookmarks).toBe(before.bookmarks + 1);
    expect(counts().days).toBe(before.days);

    advance(10_000);
    expect(counts().days).toBe(before.days + 1);
  });

  it('refreshes the day overview every ten seconds', () => {
    renderPage();
    const before = refreshDayPeaks.mock.calls.length;
    advance(10_000);
    expect(refreshDayPeaks.mock.calls.length).toBe(before + 1);
  });

  it('stops polling while the tab is hidden and resumes when it is shown', () => {
    renderPage();
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    const before = refreshRange.mock.calls.length;
    advance(10_000);
    expect(refreshRange.mock.calls.length).toBe(before);

    hidden.mockReturnValue(false);
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    expect(refreshRange.mock.calls.length).toBe(before + 1);
  });

  it('fetches the waveform once the window has settled', () => {
    renderPage();
    expect(refreshPeaks).not.toHaveBeenCalled();
    advance(180);
    expect(refreshPeaks).toHaveBeenCalledTimes(1);
  });

  it('fetches the waveform once for a drag, not once per step', () => {
    renderPage();
    advance(180);
    refreshPeaks.mockClear();

    for (let step = 1; step <= 5; step += 1) {
      act(() => {
        useTimelineStore.setState({ windowStartMs: NOW - SPAN - step * 10_000, followingLive: false });
      });
      advance(50);
    }
    expect(refreshPeaks).not.toHaveBeenCalled();

    advance(180);
    expect(refreshPeaks).toHaveBeenCalledTimes(1);
  });

  it('refetches the waveform after a zoom', () => {
    renderPage();
    advance(180);
    refreshPeaks.mockClear();

    act(() => {
      useTimelineStore.setState({ spanMs: SPAN * 2 });
    });
    advance(180);
    expect(refreshPeaks).toHaveBeenCalledTimes(1);
  });

  it('refetches the day overview when the timeline moves to another day', () => {
    renderPage();
    const before = refreshDayPeaks.mock.calls.length;

    act(() => {
      useTimelineStore.setState({ windowStartMs: NOW - 86_400_000, followingLive: false });
    });
    expect(refreshDayPeaks.mock.calls.length).toBe(before + 1);
  });

  it('does not refetch the day overview for a move within the same day', () => {
    renderPage();
    const before = refreshDayPeaks.mock.calls.length;

    act(() => {
      useTimelineStore.setState({ windowStartMs: NOW - SPAN - 600_000, followingLive: false });
    });
    expect(refreshDayPeaks.mock.calls.length).toBe(before);
  });

  it('stops polling when the page closes', () => {
    const { unmount } = renderPage();
    unmount();
    const before = refreshRange.mock.calls.length;
    advance(20_000);
    expect(refreshRange.mock.calls.length).toBe(before);
  });
});
