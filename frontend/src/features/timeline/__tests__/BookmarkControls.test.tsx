// @vitest-environment jsdom

/**
 * Adding, listing, jumping to and removing bookmarks. What matters to a person is that the bookmark lands
 * on the moment they were hearing (not the middle of the screen, and not wherever the playhead has drifted
 * to while they typed), that the list puts the newest first and takes them there, and that a listener can
 * jump but not add or remove. The store's actions are spies, so these test the component's wiring; the
 * requests themselves are the store's.
 */

import '@/test/dom';

import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import type { Bookmark, User } from '@/api/types';
import { formatClock, formatDateTime } from '@/lib/format';
import { useAuthStore } from '@/store/useAuthStore';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

import { BookmarkControls } from '../BookmarkControls';

const NOW = new Date(2026, 8, 16, 15, 30, 0).getTime();

const LISTENER: User = {
  id: 2,
  email: 'kitchen@example.com',
  role: 'listener',
  createdAtMs: NOW,
  twoFactorEnabled: false,
};

const mark = (id: number, minutesAgo: number, label: string): Bookmark => ({
  id,
  timestampMs: NOW - minutesAgo * 60_000,
  label,
  note: null,
  createdAtMs: NOW,
});

/** Oldest first, as the server returns them. */
const BOOKMARKS = [mark(1, 90, 'Parcel delivered'), mark(2, 45, 'Dog barking'), mark(3, 5, 'Phone rang')];

const pristine = {
  auth: useAuthStore.getState(),
  bookmarks: useBookmarkStore.getState(),
  timeline: useTimelineStore.getState(),
  transport: useTransportStore.getState(),
};

let add: Mock<(timestampMs: number, label: string) => Promise<Bookmark | null>>;
let remove: Mock<(id: number) => Promise<void>>;
let clearError: Mock<() => void>;
let seek: Mock<(timestampMs: number) => void>;
let focusOn: Mock<(timestampMs: number) => void>;

function renderControls(getPlayheadMs: () => number | null = () => null) {
  return render(<BookmarkControls getPlayheadMs={getPlayheadMs} />);
}

const addButton = () => screen.getByRole('button', { name: 'Add a bookmark here' });
const labelField = () => screen.getByPlaceholderText('What happened here?');
const saveButton = () => screen.getByRole('button', { name: 'Save' });

beforeEach(() => {
  useAuthStore.setState(pristine.auth, true);
  useBookmarkStore.setState(pristine.bookmarks, true);
  useTimelineStore.setState(pristine.timeline, true);
  useTransportStore.setState(pristine.transport, true);

  add = vi.fn(async (timestampMs: number, label: string): Promise<Bookmark | null> => ({
    id: 9,
    timestampMs,
    label,
    note: null,
    createdAtMs: NOW,
  }));
  remove = vi.fn(async () => undefined);
  clearError = vi.fn();
  seek = vi.fn();
  focusOn = vi.fn();

  useAuthStore.setState({ mode: 'open', user: null });
  useBookmarkStore.setState({ bookmarks: [], saving: false, error: null, add, remove, clearError });
  useTransportStore.setState({ requestedPositionMs: null, seek });
  useTimelineStore.setState({
    range: { earliestMs: NOW - 86_400_000, latestMs: NOW, liveEdgeMs: NOW, serverTimeMs: NOW, coverage: [] },
    focusOn,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('adding a bookmark', () => {
  it('opens a form stamped with the moment being heard', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW - 120_000);
    await user.click(addButton());

    expect(screen.getByText('Bookmark this moment')).toBeInTheDocument();
    expect(screen.getByText(formatDateTime(NOW - 120_000))).toBeInTheDocument();
    expect(labelField()).toHaveFocus();
    expect(clearError).toHaveBeenCalled();
  });

  it('uses the cued moment when nothing is playing', async () => {
    const user = userEvent.setup();
    useTransportStore.setState({ requestedPositionMs: NOW - 600_000 });
    renderControls(() => null);
    await user.click(addButton());
    expect(screen.getByText(formatDateTime(NOW - 600_000))).toBeInTheDocument();
  });

  it('uses the live edge when nothing is playing or cued', async () => {
    const user = userEvent.setup();
    renderControls(() => null);
    await user.click(addButton());
    expect(screen.getByText(formatDateTime(NOW))).toBeInTheDocument();
  });

  it('uses the clock when there is no live edge either', async () => {
    const user = userEvent.setup();
    vi.spyOn(Date, 'now').mockReturnValue(NOW + 7_000);
    useTimelineStore.setState({ range: null });
    renderControls(() => null);
    await user.click(addButton());
    expect(screen.getByText(formatDateTime(NOW + 7_000))).toBeInTheDocument();
  });

  it('cannot be saved without a label', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());

    expect(saveButton()).toBeDisabled();
    await user.type(labelField(), '   ');
    expect(saveButton()).toBeDisabled();
    await user.type(labelField(), 'Doorbell');
    expect(saveButton()).toBeEnabled();
  });

  it('saves the moment captured when the form opened, even though the playhead moved on', async () => {
    const user = userEvent.setup();
    let playheadMs = NOW - 120_000;
    renderControls(() => playheadMs);
    await user.click(addButton());

    playheadMs = NOW - 100_000;
    await user.type(labelField(), 'Doorbell');
    await user.click(saveButton());

    expect(add).toHaveBeenCalledWith(NOW - 120_000, 'Doorbell');
    expect(screen.queryByText('Bookmark this moment')).not.toBeInTheDocument();
  });

  it('saves on Enter', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Doorbell{Enter}');

    expect(add).toHaveBeenCalledWith(NOW, 'Doorbell');
    expect(screen.queryByText('Bookmark this moment')).not.toBeInTheDocument();
  });

  it('does not save on Enter with an empty label', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), '{Enter}');
    expect(add).not.toHaveBeenCalled();
  });

  it('stays open with the label kept when saving fails', async () => {
    const user = userEvent.setup();
    add.mockImplementation(async () => {
      useBookmarkStore.setState({ error: 'the recording there has been deleted' });
      return null;
    });
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Doorbell');
    await user.click(saveButton());

    expect(screen.getByText('Bookmark this moment')).toBeInTheDocument();
    expect(labelField()).toHaveValue('Doorbell');
    expect(screen.getByText('the recording there has been deleted')).toBeInTheDocument();
  });

  it('cannot be saved twice while a save is in flight', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Doorbell');
    act(() => {
      useBookmarkStore.setState({ saving: true });
    });
    expect(saveButton()).toBeDisabled();
  });

  it('closes without saving on Cancel', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Doorbell');
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(screen.queryByText('Bookmark this moment')).not.toBeInTheDocument();
    expect(add).not.toHaveBeenCalled();
  });

  it('closes without saving on Escape', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Doorbell{Escape}');

    expect(screen.queryByText('Bookmark this moment')).not.toBeInTheDocument();
    expect(add).not.toHaveBeenCalled();
  });

  it('starts with an empty label each time it opens', async () => {
    const user = userEvent.setup();
    renderControls(() => NOW);
    await user.click(addButton());
    await user.type(labelField(), 'Half typed');
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await user.click(addButton());
    expect(labelField()).toHaveValue('');
  });
});

describe('the bookmark list', () => {
  const listButton = (count: number) =>
    screen.getByRole('button', { name: `${count} ${count === 1 ? 'bookmark' : 'bookmarks'}` });

  it('shows how many bookmarks there are on its button', () => {
    useBookmarkStore.setState({ bookmarks: BOOKMARKS });
    renderControls();
    expect(listButton(3)).toBeInTheDocument();
  });

  it('explains itself when there are none', async () => {
    const user = userEvent.setup();
    renderControls();
    await user.click(listButton(0));

    expect(screen.getByText('No bookmarks yet')).toBeInTheDocument();
    expect(screen.getByText(/Mark a moment while listening and it appears here/)).toBeInTheDocument();
  });

  it('counts one bookmark in the singular', async () => {
    const user = userEvent.setup();
    useBookmarkStore.setState({ bookmarks: [BOOKMARKS[0]] });
    renderControls();
    await user.click(listButton(1));
    expect(screen.getByText('1 bookmark')).toBeInTheDocument();
  });

  it('lists the newest first, each with its date and time', async () => {
    const user = userEvent.setup();
    useBookmarkStore.setState({ bookmarks: BOOKMARKS });
    renderControls();
    await user.click(listButton(3));

    expect(screen.getByText('3 bookmarks')).toBeInTheDocument();
    const items = within(screen.getByRole('list')).getAllByRole('listitem');
    expect(items.map((item) => within(item).getAllByRole('button')[0].textContent)).toEqual([
      expect.stringContaining('Phone rang'),
      expect.stringContaining('Dog barking'),
      expect.stringContaining('Parcel delivered'),
    ]);
    const newest = BOOKMARKS[2].timestampMs;
    expect(items[0]).toHaveTextContent(`${formatDateTime(newest)} at ${formatClock(newest)}`);
  });

  it('jumps to a bookmark, frames it on the timeline and closes', async () => {
    const user = userEvent.setup();
    useBookmarkStore.setState({ bookmarks: BOOKMARKS });
    renderControls();
    await user.click(listButton(3));
    await user.click(screen.getByRole('button', { name: /^Dog barking/ }));

    expect(seek).toHaveBeenCalledWith(BOOKMARKS[1].timestampMs);
    expect(focusOn).toHaveBeenCalledWith(BOOKMARKS[1].timestampMs);
    expect(screen.queryByText('3 bookmarks')).not.toBeInTheDocument();
  });

  it('removes a bookmark by its own button', async () => {
    const user = userEvent.setup();
    useBookmarkStore.setState({ bookmarks: BOOKMARKS });
    renderControls();
    await user.click(listButton(3));
    await user.click(screen.getByRole('button', { name: 'Remove Dog barking' }));

    expect(remove).toHaveBeenCalledWith(2);
    expect(seek).not.toHaveBeenCalled();
  });

  it('disables removing while a change is being saved', async () => {
    const user = userEvent.setup();
    useBookmarkStore.setState({ bookmarks: BOOKMARKS, saving: true });
    renderControls();
    await user.click(listButton(3));
    expect(screen.getByRole('button', { name: 'Remove Dog barking' })).toBeDisabled();
  });
});

describe('for a listener', () => {
  beforeEach(() => {
    useAuthStore.setState({ mode: 'accounts', user: LISTENER });
    useBookmarkStore.setState({ bookmarks: BOOKMARKS });
  });

  it('offers no way to add a bookmark', () => {
    renderControls();
    expect(screen.queryByRole('button', { name: 'Add a bookmark here' })).not.toBeInTheDocument();
  });

  it('lists bookmarks without remove buttons, and still jumps to them', async () => {
    const user = userEvent.setup();
    renderControls();
    await user.click(screen.getByRole('button', { name: '3 bookmarks' }));

    expect(screen.queryByRole('button', { name: /^Remove / })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /^Phone rang/ }));
    expect(seek).toHaveBeenCalledWith(BOOKMARKS[2].timestampMs);
  });

  it('shows the add button to an admin account', () => {
    useAuthStore.setState({ user: { ...LISTENER, role: 'admin' } });
    renderControls();
    expect(addButton()).toBeInTheDocument();
  });
});
