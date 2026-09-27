// @vitest-environment jsdom

/**
 * The day picker: its label names the day on screen (today and yesterday by the recorder's clock, not the
 * browser's), the calendar lets only recorded days be chosen and only months holding recordings be
 * reached, choosing a day frames it and plays from its first moment, and the footer says what the day
 * holds. The store's actions are spies; how a day is framed is the store's, tested there.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { RecordingDay } from '@/api/types';
import { dayLabel } from '@/lib/day';
import { formatBytes, formatClock } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { DayPicker } from '../DayPicker';

const MIDDOT = '\u00b7';

/** A day that holds an hour of audio from nine in the morning. */
function recordedDay(month: number, date: number): RecordingDay {
  const dayStartMs = new Date(2026, month, date).getTime();
  const dayEndMs = new Date(2026, month, date + 1).getTime();
  const pad = (value: number) => String(value).padStart(2, '0');
  return {
    day: `2026-${pad(month + 1)}-${pad(date)}`,
    startMs: new Date(2026, month, date, 9).getTime(),
    endMs: new Date(2026, month, date, 10).getTime(),
    dayStartMs,
    dayEndMs,
    segmentCount: 360,
    bytes: 345_600_000,
    recordedMs: 3_600_000,
  };
}

const AUGUST_20 = recordedDay(7, 20);
const SEPTEMBER_15 = recordedDay(8, 15);
const SEPTEMBER_16 = recordedDay(8, 16);
/** Newest first, as the server returns them. */
const DAYS = [SEPTEMBER_16, SEPTEMBER_15, AUGUST_20];

const pristine = {
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let showDay: Mock<(day: string) => void>;
let seek: Mock<(timestampMs: number) => void>;

/** Put the middle of the timeline window at `timestampMs`. */
function viewAt(timestampMs: number) {
  useTimelineStore.setState({ windowStartMs: timestampMs - 300_000, spanMs: 600_000 });
}

/** The recorder's clock, which decides what today is. */
function serverClockAt(serverTimeMs: number) {
  useTimelineStore.setState({
    range: { earliestMs: AUGUST_20.startMs, latestMs: serverTimeMs, liveEdgeMs: serverTimeMs, serverTimeMs, coverage: [] },
  });
}

const trigger = () => screen.getByRole('button', { name: 'Choose a recorded day' });
const dayButton = (name: RegExp) => within(screen.getByRole('grid')).getByRole('button', { name });

async function openCalendar() {
  const user = userEvent.setup();
  render(<DayPicker />);
  await user.click(trigger());
  return user;
}

beforeEach(() => {
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);
  showDay = vi.fn();
  seek = vi.fn();
  useTimelineStore.setState({ days: DAYS, showDay });
  useTransportStore.setState({ seek });
  serverClockAt(new Date(2026, 8, 16, 15).getTime());
  viewAt(new Date(2026, 8, 16, 12).getTime());
});

describe('the day picker button', () => {
  it('names today by the recorder clock', () => {
    render(<DayPicker />);
    expect(trigger()).toHaveTextContent('Today');
  });

  it('names yesterday by the recorder clock, whatever the browser thinks the date is', () => {
    serverClockAt(new Date(2026, 8, 17, 8).getTime());
    render(<DayPicker />);
    expect(trigger()).toHaveTextContent('Yesterday');
  });

  it('names an older day by its date', () => {
    viewAt(new Date(2026, 7, 20, 12).getTime());
    render(<DayPicker />);
    expect(trigger()).toHaveTextContent(dayLabel('2026-08-20', null, null));
  });

  it('asks for a day when the timeline sits on no recorded day', () => {
    useTimelineStore.setState({ days: [AUGUST_20] });
    viewAt(new Date(2026, 3, 1, 12).getTime());
    render(<DayPicker />);
    expect(trigger()).toHaveTextContent('Pick a day');
  });

  it('is replaced by a note when nothing has been recorded yet', () => {
    useTimelineStore.setState({ days: [] });
    render(<DayPicker />);
    expect(screen.getByText('No recorded days yet')).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});

describe('the calendar', () => {
  it('opens on the month of the day on screen, with that day selected', async () => {
    await openCalendar();
    expect(screen.getByRole('grid', { name: 'September 2026' })).toBeInTheDocument();
    expect(dayButton(/September 16th, 2026, selected/)).toBeInTheDocument();
  });

  it('lets only the recorded days be picked', async () => {
    await openCalendar();
    const enabled = within(screen.getByRole('grid'))
      .getAllByRole('button')
      .filter((button) => !(button as HTMLButtonElement).disabled)
      .map((button) => button.textContent);
    expect(enabled).toEqual(['15', '16']);
  });

  it('frames a chosen day, plays from its first recorded moment, and closes', async () => {
    const user = await openCalendar();
    await user.click(dayButton(/September 15th/));

    expect(showDay).toHaveBeenCalledWith('2026-09-15');
    expect(seek).toHaveBeenCalledWith(SEPTEMBER_15.startMs);
    expect(screen.queryByRole('grid')).not.toBeInTheDocument();
  });

  it('does nothing when a day without audio is clicked', async () => {
    const user = await openCalendar();
    await user.click(dayButton(/September 10th/));

    expect(showDay).not.toHaveBeenCalled();
    expect(seek).not.toHaveBeenCalled();
    expect(screen.getByRole('grid')).toBeInTheDocument();
  });

  it('reaches back only as far as the oldest recording and forward only to the newest', async () => {
    const user = await openCalendar();
    const previous = () => screen.getByRole('button', { name: 'Go to the Previous Month' });
    const next = () => screen.getByRole('button', { name: 'Go to the Next Month' });

    expect(next()).toHaveAttribute('aria-disabled', 'true');
    expect(previous()).not.toHaveAttribute('aria-disabled', 'true');

    await user.click(previous());
    expect(screen.getByRole('grid', { name: 'August 2026' })).toBeInTheDocument();
    expect(previous()).toHaveAttribute('aria-disabled', 'true');

    await user.click(previous());
    expect(screen.getByRole('grid', { name: 'August 2026' })).toBeInTheDocument();
  });

  it('opens on the newest recorded month when the timeline sits on no recorded day', async () => {
    viewAt(new Date(2026, 7, 25, 12).getTime());
    await openCalendar();
    expect(screen.getByRole('grid', { name: 'September 2026' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /selected/ })).not.toBeInTheDocument();
  });
});

describe('the note under the calendar', () => {
  it('says what the day on screen holds: its span, the audio recorded and the disk it takes', async () => {
    await openCalendar();
    expect(
      screen.getByText(
        `${formatClock(SEPTEMBER_16.startMs)} to ${formatClock(SEPTEMBER_16.endMs)} ${MIDDOT} 01:00:00 recorded ${MIDDOT} ${formatBytes(SEPTEMBER_16.bytes)}`,
      ),
    ).toBeInTheDocument();
  });

  it('counts the recorded days when the timeline sits on none of them', async () => {
    viewAt(new Date(2026, 7, 25, 12).getTime());
    await openCalendar();
    expect(
      screen.getByText('3 days hold recordings. Days without audio cannot be picked.'),
    ).toBeInTheDocument();
  });

  it('counts a single recorded day in the singular', async () => {
    useTimelineStore.setState({ days: [AUGUST_20] });
    await openCalendar();
    expect(
      screen.getByText('1 day holds recordings. Days without audio cannot be picked.'),
    ).toBeInTheDocument();
  });
});
