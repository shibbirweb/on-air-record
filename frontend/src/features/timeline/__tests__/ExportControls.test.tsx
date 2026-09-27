// @vitest-environment jsdom

/**
 * Saving a span of the recording as a WAV file. The panel opens on the window the timeline is showing,
 * offers quick spans back from the newest exportable moment, asks the server what the range would produce
 * once typing settles, and says plainly when the range is backwards or refused. The server call is a spy
 * on the API client, so these test the component's wiring and wording; the plan itself is the server's.
 */

import '@/test/dom';

import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockInstance } from 'vitest';

import { api, ApiError } from '@/api/client';
import type { ExportPlan } from '@/api/types';
import { fromDateTimeLocal, toDateTimeLocal } from '@/lib/day';
import { formatBytes, formatDateTime } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';

import { ExportControls } from '../ExportControls';

const START = new Date(2026, 8, 16, 12, 0, 0).getTime();
const SPAN = 15 * 60_000;
/** The end of the last closed segment: the newest moment that can be read back. */
const EXPORTABLE_END = START + 20 * 60_000;

const plan = (fromMs: number, toMs: number, overrides: Partial<ExportPlan> = {}): ExportPlan => ({
  fromMs,
  toMs,
  durationMs: toMs - fromMs,
  sampleRate: 48_000,
  channels: 1,
  totalBytes: (toMs - fromMs) * 96,
  mixedRates: false,
  ...overrides,
});

const pristine = useTimelineStore.getState();

let exportPlan: MockInstance<(fromMs: number, toMs: number) => Promise<ExportPlan>>;

const fromField = () => screen.getByLabelText('From');
const toField = () => screen.getByLabelText('To');

/**
 * The moment a field holds. Read back rather than compared as text, because a datetime field drops
 * seconds of zero from its value, as the HTML standard says it should.
 */
const fieldMs = (field: HTMLElement) => fromDateTimeLocal((field as HTMLInputElement).value);

/**
 * Real time passing. The panel waits 400 ms after the last change before asking the server; fake timers
 * would be quicker, but they stall user-event, which waits on a real timeout between its steps.
 */
async function pause(ms: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

/** Wait out the pause after the last change, then let the answer arrive. */
async function settlePlan() {
  await pause(450);
}

async function openPanel() {
  const user = userEvent.setup();
  render(<ExportControls />);
  await user.click(screen.getByRole('button', { name: 'Export audio' }));
  return user;
}

beforeEach(() => {
  useTimelineStore.setState(pristine, true);
  useTimelineStore.setState({
    windowStartMs: START,
    spanMs: SPAN,
    range: {
      earliestMs: START - 3_600_000,
      latestMs: EXPORTABLE_END,
      liveEdgeMs: EXPORTABLE_END + 8_000,
      serverTimeMs: EXPORTABLE_END + 8_000,
      coverage: [
        { startMs: START - 3_600_000, endMs: START - 1_800_000 },
        { startMs: START - 600_000, endMs: EXPORTABLE_END },
      ],
    },
  });
  exportPlan = vi.spyOn(api, 'exportPlan').mockImplementation(async (fromMs, toMs) => plan(fromMs, toMs));
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('opening the export panel', () => {
  it('says how far the recording can be exported', async () => {
    await openPanel();
    expect(screen.getByText('Export as WAV')).toBeInTheDocument();
    expect(screen.getByText(`Available up to ${formatDateTime(EXPORTABLE_END)}`)).toBeInTheDocument();
  });

  it('starts from the window the timeline is showing', async () => {
    await openPanel();
    expect(fieldMs(fromField())).toBe(START);
    expect(fieldMs(toField())).toBe(START + SPAN);
  });

  it('bounds both fields by the oldest recording and the newest exportable moment', async () => {
    await openPanel();
    for (const field of [fromField(), toField()]) {
      expect(field).toHaveAttribute('min', toDateTimeLocal(START - 3_600_000));
      expect(field).toHaveAttribute('max', toDateTimeLocal(EXPORTABLE_END));
      expect(field).toHaveAttribute('type', 'datetime-local');
    }
  });

  it('keeps the range it opened with while following live moves the timeline on', async () => {
    await openPanel();
    act(() => {
      useTimelineStore.setState({ windowStartMs: START + 60_000 });
    });
    expect(fieldMs(fromField())).toBe(START);
  });

  it('says so when nothing has been recorded, and offers no recent spans', async () => {
    useTimelineStore.setState({
      range: { earliestMs: null, latestMs: null, liveEdgeMs: null, serverTimeMs: START, coverage: [] },
    });
    await openPanel();
    expect(screen.getByText('Nothing has been recorded yet.')).toBeInTheDocument();
    for (const label of ['Last 1m', 'Last 5m', 'Last 15m']) {
      expect(screen.getByRole('button', { name: label })).toBeDisabled();
    }
    expect(screen.getByRole('button', { name: 'Visible window' })).toBeEnabled();
  });
});

describe('the plan for the range', () => {
  it('is asked for once the range settles, and shows the length, format and size', async () => {
    await openPanel();
    expect(exportPlan).not.toHaveBeenCalled();
    await settlePlan();

    expect(exportPlan).toHaveBeenCalledWith(START, START + SPAN);
    expect(screen.getByText('Length').nextElementSibling).toHaveTextContent('00:15:00');
    expect(screen.getByText('Format').nextElementSibling).toHaveTextContent('48.0 kHz, mono, 16 bit');
    expect(screen.getByText('File size').nextElementSibling).toHaveTextContent(formatBytes(SPAN * 96));
    expect(screen.getByText(/Gaps are exported as silence/)).toBeInTheDocument();
  });

  it('shows that it is working while the server answers', async () => {
    let answer: (value: ExportPlan) => void = () => undefined;
    exportPlan.mockImplementation(() => new Promise<ExportPlan>((resolve) => (answer = resolve)));
    await openPanel();
    await settlePlan();
    expect(screen.getByText('Working out the size...')).toBeInTheDocument();

    await act(async () => {
      answer(plan(START, START + SPAN));
    });
    expect(screen.queryByText('Working out the size...')).not.toBeInTheDocument();
    expect(screen.getByText('Length')).toBeInTheDocument();
  });

  it('names the channel count for a range recorded in stereo', async () => {
    exportPlan.mockImplementation(async (fromMs, toMs) => plan(fromMs, toMs, { channels: 2, sampleRate: 44_100 }));
    await openPanel();
    await settlePlan();
    expect(screen.getByText('Format').nextElementSibling).toHaveTextContent('44.1 kHz, 2 ch, 16 bit');
  });

  it('warns when the range mixes recording rates', async () => {
    exportPlan.mockImplementation(async (fromMs, toMs) => plan(fromMs, toMs, { mixedRates: true }));
    await openPanel();
    await settlePlan();
    expect(screen.getByText(/recorded at more than one bit rate/)).toBeInTheDocument();
  });

  it('offers a download link for exactly the planned range, which closes the panel', async () => {
    const user = await openPanel();
    await settlePlan();

    const link = screen.getByRole('link', { name: `Download ${formatBytes(SPAN * 96)}` });
    expect(link).toHaveAttribute('href', api.exportUrl(START, START + SPAN));
    expect(link).toHaveAttribute('download');

    // jsdom cannot follow a link, so the navigation itself is stopped once the component has seen the click.
    const stopNavigation = (event: Event) => event.preventDefault();
    document.addEventListener('click', stopNavigation);
    await user.click(link);
    document.removeEventListener('click', stopNavigation);

    expect(screen.queryByText('Export as WAV')).not.toBeInTheDocument();
  });

  it("shows the server's own words when it refuses the range", async () => {
    exportPlan.mockRejectedValue(new ApiError('that range holds no recording', 'empty_range', 422));
    await openPanel();
    await settlePlan();

    expect(screen.getByText('that range holds no recording')).toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('falls back to a plain message when the server cannot be reached', async () => {
    exportPlan.mockRejectedValue(new TypeError('Failed to fetch'));
    await openPanel();
    await settlePlan();
    expect(screen.getByText('could not work out the export')).toBeInTheDocument();
  });

  it('is not asked for while the panel is closed', async () => {
    render(<ExportControls />);
    await settlePlan();
    expect(exportPlan).not.toHaveBeenCalled();
  });
});

describe('choosing the range', () => {
  it('takes the most recent minutes from the newest exportable moment', async () => {
    const user = await openPanel();
    await user.click(screen.getByRole('button', { name: 'Last 5m' }));

    expect(fieldMs(fromField())).toBe(EXPORTABLE_END - 5 * 60_000);
    expect(fieldMs(toField())).toBe(EXPORTABLE_END);
    await settlePlan();
    expect(exportPlan).toHaveBeenLastCalledWith(EXPORTABLE_END - 5 * 60_000, EXPORTABLE_END);
  });

  it('offers the last minute and the last quarter hour too', async () => {
    const user = await openPanel();
    await user.click(screen.getByRole('button', { name: 'Last 1m' }));
    expect(fieldMs(fromField())).toBe(EXPORTABLE_END - 60_000);
    await user.click(screen.getByRole('button', { name: 'Last 15m' }));
    expect(fieldMs(fromField())).toBe(EXPORTABLE_END - 15 * 60_000);
  });

  it('goes back to the visible window', async () => {
    const user = await openPanel();
    await user.click(screen.getByRole('button', { name: 'Last 1m' }));
    await user.click(screen.getByRole('button', { name: 'Visible window' }));
    expect(fieldMs(fromField())).toBe(START);
    expect(fieldMs(toField())).toBe(START + SPAN);
  });

  it('takes typed times, asking the server once when the typing stops', async () => {
    await openPanel();
    fireEvent.change(fromField(), { target: { value: toDateTimeLocal(START + 60_000) } });
    await pause(150);
    fireEvent.change(fromField(), { target: { value: toDateTimeLocal(START + 120_000) } });
    await settlePlan();

    expect(exportPlan).toHaveBeenCalledTimes(1);
    expect(exportPlan).toHaveBeenCalledWith(START + 120_000, START + SPAN);
  });

  it('ignores a field cleared part way through typing', async () => {
    await openPanel();
    fireEvent.change(toField(), { target: { value: '' } });
    expect(fieldMs(toField())).toBe(START + SPAN);
  });

  it('refuses a range that ends before it starts, without asking the server', async () => {
    await openPanel();
    fireEvent.change(toField(), { target: { value: toDateTimeLocal(START - 60_000) } });
    await settlePlan();

    expect(screen.getByText('The end of the range must be after the start.')).toBeInTheDocument();
    expect(exportPlan).not.toHaveBeenCalled();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('hides a plan made for an earlier range once the range turns backwards', async () => {
    await openPanel();
    await settlePlan();
    expect(screen.getByRole('link')).toBeInTheDocument();

    fireEvent.change(toField(), { target: { value: toDateTimeLocal(START) } });
    expect(screen.getByText('The end of the range must be after the start.')).toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('starts afresh from the current window each time it opens', async () => {
    const user = await openPanel();
    await user.click(screen.getByRole('button', { name: 'Last 1m' }));
    await user.keyboard('{Escape}');
    expect(screen.queryByText('Export as WAV')).not.toBeInTheDocument();

    act(() => {
      useTimelineStore.setState({ windowStartMs: START - 3_000_000 });
    });
    await user.click(screen.getByRole('button', { name: 'Export audio' }));
    expect(fieldMs(fromField())).toBe(START - 3_000_000);
  });
});
