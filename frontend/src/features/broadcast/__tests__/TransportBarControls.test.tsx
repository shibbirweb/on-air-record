// @vitest-environment jsdom

/**
 * The transport controls other than the previous and next sound buttons, which TransportBar.test.tsx
 * covers: play and pause, back thirty seconds, go live, the clock and the badge beside it that says
 * whether this is the present or how far behind it, playback speed, and volume and mute. The stores'
 * actions are spies, so these test the component's wiring; what reaches the socket is the store's.
 *
 * The clock is sampled on animation frames rather than rendered from state, so the frames are cranked by
 * hand here and the playhead is read through the same callback the audio clock feeds.
 */

import '@/test/dom';

import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { TooltipProvider } from '@/components/ui/tooltip';
import { formatClock } from '@/lib/format';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { TransportBar } from '../TransportBar';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();

const pristine = {
  connection: useConnectionStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let play: Mock<() => Promise<void>>;
let pause: Mock<() => void>;
let goLive: Mock<() => void>;
let seek: Mock<(timestampMs: number) => void>;
let setVolume: Mock<(volume: number) => void>;
let toggleMuted: Mock<() => void>;
let requestSpeed: Mock<(speed: number) => void>;

let queuedFrames: FrameRequestCallback[] = [];

/** Run the queued animation frames once, as the browser would on its next paint. */
function frame() {
  act(() => {
    const running = queuedFrames;
    queuedFrames = [];
    for (const callback of running) {
      callback(performance.now());
    }
  });
}

function renderBar(getPlayheadMs: () => number | null = () => null) {
  return render(
    <TooltipProvider>
      <TransportBar getPlayheadMs={getPlayheadMs} />
    </TooltipProvider>,
  );
}

function withLiveEdge(liveEdgeMs: number | null) {
  useTimelineStore.setState({
    range:
      liveEdgeMs === null
        ? null
        : { earliestMs: NOW - 86_400_000, latestMs: liveEdgeMs, liveEdgeMs, serverTimeMs: liveEdgeMs, coverage: [] },
  });
}

/** The badge after the clock, which says live, cued, or how far behind. */
const badge = (text: string) => screen.getByText(text, { selector: '[data-slot="badge"]' });
const speedControl = () => screen.getByRole('combobox', { name: 'Playback speed' });

beforeEach(() => {
  useConnectionStore.setState(pristine.connection, true);
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);

  queuedFrames = [];
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
    queuedFrames.push(callback);
    return queuedFrames.length;
  });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);

  play = vi.fn(async () => undefined);
  pause = vi.fn();
  goLive = vi.fn();
  seek = vi.fn();
  setVolume = vi.fn();
  toggleMuted = vi.fn();
  requestSpeed = vi.fn();

  useConnectionStore.setState({ connected: true });
  useTransportStore.setState({
    playing: false,
    mode: 'live',
    volume: 0.8,
    muted: false,
    speed: 1,
    endOfRecording: false,
    requestedPositionMs: null,
    play,
    pause,
    goLive,
    seek,
    setVolume,
    toggleMuted,
    requestSpeed,
  });
  withLiveEdge(NOW);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('play and pause', () => {
  it('plays when stopped', async () => {
    const user = userEvent.setup();
    renderBar();
    await user.click(screen.getByRole('button', { name: 'Play' }));
    expect(play).toHaveBeenCalledTimes(1);
    expect(pause).not.toHaveBeenCalled();
  });

  it('pauses while playing', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ playing: true });
    renderBar(() => NOW);
    await user.click(screen.getByRole('button', { name: 'Pause' }));
    expect(pause).toHaveBeenCalledTimes(1);
    expect(play).not.toHaveBeenCalled();
  });

  it('cannot be pressed while the broadcast link is down', () => {
    useConnectionStore.setState({ connected: false });
    renderBar();
    expect(screen.getByRole('button', { name: 'Play' })).toBeDisabled();
  });
});

describe('back 30 seconds', () => {
  it('jumps half a minute behind what is playing', async () => {
    const user = userEvent.setup();
    renderBar(() => NOW - 100_000);
    await user.click(screen.getByRole('button', { name: 'Back 30 seconds' }));
    expect(seek).toHaveBeenCalledWith(NOW - 130_000);
  });

  it('jumps half a minute behind the live edge when nothing plays', async () => {
    const user = userEvent.setup();
    renderBar(() => null);
    await user.click(screen.getByRole('button', { name: 'Back 30 seconds' }));
    expect(seek).toHaveBeenCalledWith(NOW - 30_000);
  });

  it('falls back to the clock when there is no live edge yet', async () => {
    const user = userEvent.setup();
    withLiveEdge(null);
    vi.spyOn(Date, 'now').mockReturnValue(NOW + 5_000);
    renderBar(() => null);
    await user.click(screen.getByRole('button', { name: 'Back 30 seconds' }));
    expect(seek).toHaveBeenCalledWith(NOW + 5_000 - 30_000);
  });

  it('explains itself on hover', async () => {
    const user = userEvent.setup();
    renderBar();
    await user.hover(screen.getByRole('button', { name: 'Back 30 seconds' }));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Jump back 30 seconds');
  });
});

describe('go live', () => {
  it('returns to the present', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => NOW - 600_000);
    await user.click(screen.getByRole('button', { name: 'Go live' }));
    expect(goLive).toHaveBeenCalledTimes(1);
  });

  it('stands out while listening to the past', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => NOW - 600_000);
    frame();
    expect(screen.getByRole('button', { name: 'Go live' })).toHaveClass('bg-primary');
  });

  it('stands out once the live feed has fallen behind the edge', () => {
    useTransportStore.setState({ playing: true, mode: 'live' });
    renderBar(() => NOW - 10_000);
    frame();
    expect(screen.getByRole('button', { name: 'Go live' })).toHaveClass('bg-primary');
  });

  it('stays quiet while already live', () => {
    useTransportStore.setState({ playing: true, mode: 'live' });
    renderBar(() => NOW - 500);
    frame();
    expect(screen.getByRole('button', { name: 'Go live' })).toHaveClass('bg-secondary');
  });

  it('cannot be pressed while the broadcast link is down', () => {
    useConnectionStore.setState({ connected: false });
    renderBar();
    expect(screen.getByRole('button', { name: 'Go live' })).toBeDisabled();
  });
});

describe('the clock', () => {
  it('shows dashes before anything plays or is cued', () => {
    renderBar(() => null);
    frame();
    expect(screen.getByText('--:--:--')).toBeInTheDocument();
  });

  it('shows the time of the audio being heard, from the audio clock', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => NOW - 254_000);
    frame();
    expect(screen.getByText(formatClock(NOW - 254_000))).toBeInTheDocument();
  });

  it('shows the cued moment until the audio clock has something better', () => {
    useTransportStore.setState({ requestedPositionMs: NOW - 3_600_000 });
    renderBar(() => null);
    frame();
    expect(screen.getByText(formatClock(NOW - 3_600_000))).toBeInTheDocument();
  });

  it('moves with the audio clock from frame to frame', () => {
    let playheadMs = NOW - 10_000;
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => playheadMs);
    frame();
    expect(screen.getByText(formatClock(NOW - 10_000))).toBeInTheDocument();

    playheadMs = NOW - 7_000;
    frame();
    expect(screen.getByText(formatClock(NOW - 7_000))).toBeInTheDocument();
  });
});

describe('the badge beside the clock', () => {
  it('says not playing before play is pressed', () => {
    renderBar();
    frame();
    expect(badge('not playing')).toBeInTheDocument();
  });

  it('says cued when a moment is waiting for play', () => {
    useTransportStore.setState({ requestedPositionMs: NOW - 60_000 });
    renderBar();
    frame();
    expect(badge('cued')).toBeInTheDocument();
  });

  it('says on air while playing the present', () => {
    useTransportStore.setState({ playing: true, mode: 'live' });
    renderBar(() => NOW - 800);
    frame();
    expect(badge('on air')).toBeInTheDocument();
  });

  it('says how far behind the present while playing the past', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => NOW - 254_000);
    frame();
    expect(badge('4m 14s behind')).toBeInTheDocument();
    expect(screen.queryByText('on air')).not.toBeInTheDocument();
  });

  it('counts hours for a long way back', () => {
    useTransportStore.setState({ playing: true, mode: 'playback' });
    renderBar(() => NOW - (3 * 3_600_000 + 20 * 60_000));
    frame();
    expect(badge('3h 20m behind')).toBeInTheDocument();
  });

  it('stops saying on air once the live feed has drifted behind the edge', () => {
    useTransportStore.setState({ playing: true, mode: 'live' });
    renderBar(() => NOW - 12_000);
    frame();
    expect(badge('12s behind')).toBeInTheDocument();
  });

  it('adds end of recording when playback has run out of audio', () => {
    useTransportStore.setState({ playing: true, mode: 'playback', endOfRecording: true });
    renderBar(() => NOW - 60_000);
    frame();
    expect(badge('end of recording')).toBeInTheDocument();
  });
});

describe('playback speed', () => {
  it('shows the speed in force', () => {
    useTransportStore.setState({ mode: 'playback', speed: 0.5 });
    renderBar();
    expect(speedControl()).toHaveTextContent('0.5x');
  });

  it('is greyed out while live, since the present cannot be played faster', () => {
    renderBar();
    expect(speedControl()).toBeDisabled();
    expect(speedControl()).toHaveTextContent('1x');
  });

  it('is greyed out while the broadcast link is down', () => {
    useConnectionStore.setState({ connected: false });
    useTransportStore.setState({ mode: 'playback' });
    renderBar();
    expect(speedControl()).toBeDisabled();
  });

  it('offers a quarter speed to four times, with normal marked', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ mode: 'playback' });
    renderBar();
    await user.click(speedControl());
    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual([
      '0.25x',
      '0.5x',
      '1x (normal)',
      '1.5x',
      '2x',
      '4x',
    ]);
  });

  it('asks for the chosen speed', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ mode: 'playback' });
    renderBar();
    await user.click(speedControl());
    await user.click(screen.getByRole('option', { name: '2x' }));
    expect(requestSpeed).toHaveBeenCalledWith(2);
  });

  it('works while paused in the past', () => {
    useTransportStore.setState({ mode: 'paused' });
    renderBar();
    expect(speedControl()).toBeEnabled();
  });
});

describe('volume and mute', () => {
  const volumeSlider = () => screen.getByRole('slider', { name: 'Volume' });

  it('shows the volume on the slider', () => {
    renderBar();
    expect(volumeSlider()).toHaveAttribute('aria-valuenow', '0.8');
  });

  it('changes the volume from the keyboard', async () => {
    const user = userEvent.setup();
    renderBar();
    volumeSlider().focus();
    await user.keyboard('{ArrowRight}');
    expect(setVolume).toHaveBeenCalledTimes(1);
    expect(setVolume.mock.calls[0][0]).toBeCloseTo(0.81);
  });

  it('mutes', async () => {
    const user = userEvent.setup();
    renderBar();
    await user.click(screen.getByRole('button', { name: 'Mute' }));
    expect(toggleMuted).toHaveBeenCalledTimes(1);
  });

  it('unmutes, and shows the slider at zero while muted', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ muted: true });
    renderBar();
    expect(volumeSlider()).toHaveAttribute('aria-valuenow', '0');
    await user.click(screen.getByRole('button', { name: 'Unmute' }));
    expect(toggleMuted).toHaveBeenCalledTimes(1);
  });

  it('shows the silent speaker when muted or turned all the way down', () => {
    useTransportStore.setState({ volume: 0 });
    const { unmount } = renderBar();
    expect(screen.getByRole('button', { name: 'Mute' }).querySelector('svg')).toHaveClass('lucide-volume-x');
    unmount();

    useTransportStore.setState({ volume: 0.8 });
    renderBar();
    expect(screen.getByRole('button', { name: 'Mute' }).querySelector('svg')).toHaveClass('lucide-volume-2');
  });
});
