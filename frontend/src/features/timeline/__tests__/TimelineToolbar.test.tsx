// @vitest-environment jsdom

/**
 * The toolbar above the timeline: the reset and zoom buttons, the preset spans, the follow live toggle and
 * the history badge, plus the three panels it hosts (day picker, bookmarks, export), each tested in its own
 * file. Zooming holds whatever is playing, or the cued moment, in place, so each button is checked for the
 * anchor it hands the store. The store's actions are spies; the geometry is the store's, tested there.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { TooltipProvider } from '@/components/ui/tooltip';
import { formatDateTime } from '@/lib/format';
import { useAuthStore } from '@/store/useAuthStore';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore, ZOOM_LEVELS } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { TimelineToolbar } from '../TimelineToolbar';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();
const FIFTEEN_MINUTES = 15 * 60_000;
const PRESETS = ['1m', '5m', '15m', '1h', '4h', '12h', '24h'];

const pristine = {
  auth: useAuthStore.getState(),
  bookmarks: useBookmarkStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let zoomTo: Mock<(spanMs: number, anchorMs?: number | null) => void>;
let resetView: Mock<(anchorMs?: number | null) => void>;
let setFollowingLive: Mock<(following: boolean) => void>;

function renderToolbar(getPlayheadMs: () => number | null = () => null) {
  return render(
    <TooltipProvider>
      <TimelineToolbar getPlayheadMs={getPlayheadMs} />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  useAuthStore.setState(pristine.auth, true);
  useBookmarkStore.setState(pristine.bookmarks, true);
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);

  zoomTo = vi.fn();
  resetView = vi.fn();
  setFollowingLive = vi.fn();
  useAuthStore.setState({ mode: 'open', user: null });
  useTransportStore.setState({ requestedPositionMs: null });
  useTimelineStore.setState({
    windowStartMs: NOW - FIFTEEN_MINUTES,
    spanMs: FIFTEEN_MINUTES,
    followingLive: true,
    days: [],
    range: {
      earliestMs: NOW - 86_400_000,
      latestMs: NOW,
      liveEdgeMs: NOW,
      serverTimeMs: NOW,
      coverage: [{ startMs: NOW - 86_400_000, endMs: NOW - 5_000 }],
    },
    zoomTo,
    resetView,
    setFollowingLive,
  });
});

describe('the panels the toolbar hosts', () => {
  it('holds the day picker, the bookmark controls and the export button', () => {
    useTimelineStore.setState({
      days: [
        {
          day: '2026-09-16',
          startMs: NOW - 3_600_000,
          endMs: NOW,
          dayStartMs: new Date(2026, 8, 16).getTime(),
          dayEndMs: new Date(2026, 8, 17).getTime(),
          segmentCount: 1,
          bytes: 1,
          recordedMs: 3_600_000,
        },
      ],
    });
    renderToolbar();
    expect(screen.getByRole('button', { name: 'Choose a recorded day' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add a bookmark here' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Export audio' })).toBeInTheDocument();
  });

  it('keeps export for a listener but not adding bookmarks', () => {
    useAuthStore.setState({
      mode: 'accounts',
      user: { id: 2, email: 'kitchen@example.com', role: 'listener', createdAtMs: NOW, twoFactorEnabled: false },
    });
    renderToolbar();
    expect(screen.getByRole('button', { name: 'Export audio' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a bookmark here' })).not.toBeInTheDocument();
  });
});

describe('the reset button', () => {
  it('resets the view around what is playing', async () => {
    const user = userEvent.setup();
    renderToolbar(() => NOW - 600_000);
    await user.click(screen.getByRole('button', { name: 'Reset the view' }));
    expect(resetView).toHaveBeenCalledWith(NOW - 600_000);
  });

  it('resets around the cued moment when nothing plays', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ requestedPositionMs: NOW - 7_200_000 });
    renderToolbar(() => null);
    await user.click(screen.getByRole('button', { name: 'Reset the view' }));
    expect(resetView).toHaveBeenCalledWith(NOW - 7_200_000);
  });

  it('resets with no anchor when nothing plays or is cued, which means the live view', async () => {
    const user = userEvent.setup();
    renderToolbar(() => null);
    await user.click(screen.getByRole('button', { name: 'Reset the view' }));
    expect(resetView).toHaveBeenCalledWith(null);
  });

  it('explains itself on hover', async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.hover(screen.getByRole('button', { name: 'Reset the view' }));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Reset to the standard view');
  });
});

describe('the zoom buttons', () => {
  it('zoom in to half the span, holding what is playing in place', async () => {
    const user = userEvent.setup();
    renderToolbar(() => NOW - 60_000);
    await user.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(zoomTo).toHaveBeenCalledWith(FIFTEEN_MINUTES / 2, NOW - 60_000);
  });

  it('zoom out to twice the span, holding the cued moment when nothing plays', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ requestedPositionMs: NOW - 300_000 });
    renderToolbar(() => null);
    await user.click(screen.getByRole('button', { name: 'Zoom out' }));
    expect(zoomTo).toHaveBeenCalledWith(FIFTEEN_MINUTES * 2, NOW - 300_000);
  });

  it('zoom about the middle of the view when nothing plays or is cued', async () => {
    const user = userEvent.setup();
    renderToolbar(() => null);
    await user.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(zoomTo).toHaveBeenCalledWith(FIFTEEN_MINUTES / 2, null);
  });

  it('read the playhead at the moment of the click, not when the toolbar last drew', async () => {
    const user = userEvent.setup();
    let playheadMs = NOW - 60_000;
    renderToolbar(() => playheadMs);
    playheadMs = NOW - 50_000;
    await user.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(zoomTo).toHaveBeenCalledWith(FIFTEEN_MINUTES / 2, NOW - 50_000);
  });
});

describe('the preset spans', () => {
  it('offer one minute to a whole day', () => {
    renderToolbar();
    for (const label of PRESETS) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument();
    }
  });

  it('each zoom straight to their span, holding what is playing', async () => {
    const user = userEvent.setup();
    renderToolbar(() => NOW - 1_000);
    for (const [index, label] of PRESETS.entries()) {
      await user.click(screen.getByRole('button', { name: label }));
      expect(zoomTo).toHaveBeenLastCalledWith(ZOOM_LEVELS[index], NOW - 1_000);
    }
  });

  it('highlight the one matching the current span', () => {
    renderToolbar();
    expect(screen.getByRole('button', { name: '15m' })).toHaveClass('bg-secondary');
    expect(screen.getByRole('button', { name: '1h' })).not.toHaveClass('bg-secondary');
  });

  it('highlight none when the span sits between presets', () => {
    useTimelineStore.setState({ spanMs: 40 * 60_000 });
    renderToolbar();
    for (const label of PRESETS) {
      expect(screen.getByRole('button', { name: label })).not.toHaveClass('bg-secondary');
    }
  });
});

describe('the follow live toggle', () => {
  it('reads Following live while on, and turns it off', async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.click(screen.getByRole('button', { name: 'Following live' }));
    expect(setFollowingLive).toHaveBeenCalledWith(false);
  });

  it('reads Follow live while off, and turns it on', async () => {
    const user = userEvent.setup();
    useTimelineStore.setState({ followingLive: false });
    renderToolbar();
    await user.click(screen.getByRole('button', { name: 'Follow live' }));
    expect(setFollowingLive).toHaveBeenCalledWith(true);
  });
});

describe('the history badge', () => {
  it('says how far back the recording reaches', () => {
    renderToolbar();
    expect(screen.getByText(`history from ${formatDateTime(NOW - 86_400_000)}`)).toBeInTheDocument();
  });

  it('says when nothing has been recorded yet', () => {
    useTimelineStore.setState({ range: null });
    renderToolbar();
    expect(screen.getByText('nothing recorded yet')).toBeInTheDocument();
  });
});
