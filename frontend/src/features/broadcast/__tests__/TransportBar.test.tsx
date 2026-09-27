// @vitest-environment jsdom

/**
 * The previous and next sound buttons: what they ask the server for, what they do with the answer, and
 * what they say when there is nothing. The stores' actions are replaced with spies, so these test the
 * component's wiring; the search itself is the server's, tested there, and the whole path is driven in a
 * real browser by scripts/e2e.mjs.
 */

import '@/test/dom';

import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Sound } from '@/api/types';
import { TooltipProvider } from '@/components/ui/tooltip';
import { noSoundNotice } from '@/lib/soundSearch';
import type { SoundSearchDirection } from '@/lib/soundSearch';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { TransportBar } from '../TransportBar';

const NOW = 1_757_034_000_000;
const SOUND: Sound = { startMs: NOW - 600_000, endMs: NOW - 597_000, seekMs: NOW - 601_000, peak: 80 };

let findSound: Mock<(fromMs: number, direction: SoundSearchDirection) => Promise<Sound | null>>;
let bringIntoView: Mock<(timestampMs: number) => void>;
let seek: Mock<(timestampMs: number) => void>;

function renderBar(playheadMs: number | null = null) {
  return render(
    <TooltipProvider>
      <TransportBar getPlayheadMs={() => playheadMs} />
    </TooltipProvider>,
  );
}

const nextButton = () => screen.getByRole('button', { name: 'Next sound' });
const previousButton = () => screen.getByRole('button', { name: 'Previous sound' });

/** Let a pending search resolve and React settle. */
async function settle() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe('the previous and next sound buttons', () => {
  beforeEach(() => {
    findSound = vi.fn(async (): Promise<Sound | null> => SOUND);
    bringIntoView = vi.fn<(timestampMs: number) => void>();
    seek = vi.fn<(timestampMs: number) => void>();
    useConnectionStore.setState({ connected: true });
    useTransportStore.setState({ playing: false, requestedPositionMs: null, seek });
    useTimelineStore.setState({
      range: { earliestMs: NOW - 86_400_000, latestMs: NOW, liveEdgeMs: NOW, serverTimeMs: NOW, coverage: [] },
      windowStartMs: NOW - 3_600_000,
      followingLive: true,
      findSound,
      bringIntoView,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('are both there and ready', () => {
    renderBar();
    expect(previousButton()).toBeEnabled();
    expect(nextButton()).toBeEnabled();
  });

  it('search forward from what is playing, then seek to the sound and bring it into view', async () => {
    renderBar(NOW - 900_000);
    fireEvent.click(nextButton());
    await settle();

    expect(findSound).toHaveBeenCalledWith(NOW - 900_000, 'forward');
    expect(seek).toHaveBeenCalledWith(SOUND.seekMs);
    expect(bringIntoView).toHaveBeenCalledWith(SOUND.startMs);
  });

  it('search backward for previous', async () => {
    renderBar(NOW - 100_000);
    fireEvent.click(previousButton());
    await settle();
    expect(findSound).toHaveBeenCalledWith(NOW - 100_000, 'backward');
  });

  it('search from the cued moment when nothing plays', async () => {
    useTransportStore.setState({ requestedPositionMs: NOW - 1_200_000 });
    renderBar(null);
    fireEvent.click(nextButton());
    await settle();
    expect(findSound).toHaveBeenCalledWith(NOW - 1_200_000, 'forward');
  });

  it('search from the left edge of the timeline when browsing it without playing', async () => {
    useTimelineStore.setState({ followingLive: false, windowStartMs: NOW - 7_200_000 });
    renderBar(null);
    fireEvent.click(nextButton());
    await settle();
    expect(findSound).toHaveBeenCalledWith(NOW - 7_200_000, 'forward');
  });

  it('are both disabled while a search is running, and ready again after', async () => {
    let answer: (sound: Sound | null) => void = () => undefined;
    findSound.mockImplementation(() => new Promise<Sound | null>((resolve) => (answer = resolve)));
    renderBar(NOW - 900_000);

    fireEvent.click(nextButton());
    expect(nextButton()).toBeDisabled();
    expect(previousButton()).toBeDisabled();
    // A second press while busy starts nothing.
    fireEvent.click(previousButton());
    expect(findSound).toHaveBeenCalledTimes(1);

    await act(async () => {
      answer(SOUND);
    });
    expect(nextButton()).toBeEnabled();
    expect(previousButton()).toBeEnabled();
  });

  it('say when there is no later sound, do not seek, and let the note go after a few seconds', async () => {
    vi.useFakeTimers();
    findSound.mockResolvedValue(null);
    renderBar(NOW - 900_000);

    fireEvent.click(nextButton());
    await settle();
    expect(screen.getByRole('status')).toHaveTextContent(noSoundNotice('forward'));
    expect(seek).not.toHaveBeenCalled();
    expect(bringIntoView).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(4_100);
    });
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('say when there is no earlier sound', async () => {
    findSound.mockResolvedValue(null);
    renderBar(NOW - 900_000);
    fireEvent.click(previousButton());
    await settle();
    expect(screen.getByRole('status')).toHaveTextContent(noSoundNotice('backward'));
  });

  it('say so plainly when the search fails, and are ready to try again', async () => {
    findSound.mockRejectedValue(new Error('the service is restarting'));
    renderBar(NOW - 900_000);
    fireEvent.click(nextButton());
    await settle();
    expect(screen.getByRole('status')).toHaveTextContent('Could not search for sounds');
    expect(seek).not.toHaveBeenCalled();
    expect(nextButton()).toBeEnabled();
  });

  it('clear an old note when a new search starts', async () => {
    findSound.mockResolvedValueOnce(null).mockResolvedValueOnce(SOUND);
    renderBar(NOW - 900_000);
    fireEvent.click(nextButton());
    await settle();
    expect(screen.getByRole('status')).toBeInTheDocument();

    fireEvent.click(previousButton());
    await settle();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(seek).toHaveBeenCalledWith(SOUND.seekMs);
  });
});
