// @vitest-environment jsdom

/**
 * The listener count in the top bar, and the list of who is listening behind it for those allowed to see
 * it. Covers the count in both of its sources, opening by hover with a grace period, by tap and by
 * keyboard, and what the list says about each person and each tab: the grouping, the viewer's own entry
 * first, guests, what each tab is doing, its browser and address, and how long it has been connected. The
 * wording helpers are tested on their own in lib/__tests__/listeners.test.ts; this is the wiring.
 */

import '@/test/dom';

import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ListenerView, ServiceStatus } from '@/api/types';
import { formatClock, formatDateTime } from '@/lib/format';
import { useAuthStore } from '@/store/useAuthStore';
import { useListenersStore } from '@/store/useListenersStore';
import { useStatusStore } from '@/store/useStatusStore';

import { ListenersBadge } from '../ListenersBadge';

const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();
const EARLIER_TODAY = new Date(2026, 8, 16, 7, 35, 43).getTime();

const MAC_CHROME =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36';
const IPHONE_SAFARI =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1';
const WINDOWS_FIREFOX = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:130.0) Gecko/20100101 Firefox/130.0';

const listener = (overrides: Partial<ListenerView> & Pick<ListenerView, 'id'>): ListenerView => ({
  email: null,
  role: null,
  address: '192.168.1.10',
  userAgent: MAC_CHROME,
  connectedAtMs: NOW,
  activity: 'live',
  fromMs: null,
  player: 'playing',
  ...overrides,
});

const OWNER = listener({
  id: 1,
  email: 'owner@example.com',
  role: 'admin',
  connectedAtMs: NOW - 30 * 60_000,
});
const KITCHEN_PHONE = listener({
  id: 2,
  email: 'kitchen@example.com',
  role: 'listener',
  address: '192.168.1.20',
  userAgent: IPHONE_SAFARI,
  connectedAtMs: NOW - 5 * 60_000,
  activity: 'playback',
  fromMs: EARLIER_TODAY,
});
const KITCHEN_TAB = listener({
  id: 4,
  email: 'kitchen@example.com',
  role: 'listener',
  address: '192.168.1.20',
  userAgent: null,
  connectedAtMs: NOW - 10_000,
  player: 'idle',
});
const GUEST = listener({
  id: 3,
  address: '192.168.1.30',
  userAgent: WINDOWS_FIREFOX,
  connectedAtMs: NOW - 2 * 3_600_000,
  player: 'paused',
});

/** Arrival order is guest, owner, kitchen; the owner is the viewer and comes first regardless. */
const LISTENERS = [KITCHEN_PHONE, GUEST, OWNER, KITCHEN_TAB];

const STATUS: ServiceStatus = {
  capture: {
    state: 'recording',
    sessionId: 7,
    deviceId: 'mic',
    deviceName: 'USB microphone',
    sampleRate: 48_000,
    deviceSampleRate: 48_000,
    channels: 1,
    frameMs: 100,
    startedAtMs: NOW - 3_600_000,
    droppedFrames: 0,
    error: null,
  },
  levels: { rms: 0, peak: 0 },
  listeners: 3,
  serverTimeMs: NOW,
  liveEdgeMs: NOW,
};

const pristine = {
  auth: useAuthStore.getState(),
  listeners: useListenersStore.getState(),
  status: useStatusStore.getState(),
};

const trigger = (count = 4) =>
  screen.getByRole('button', { name: `${count} listeners, show who is listening` });
const heading = () => screen.queryByText('Listening now');

/** The row for one tab, found by the browser line under it. */
const tabRow = (browserLine: string) => {
  const row = screen.getByText(browserLine, { exact: false }).closest('li');
  if (!row) {
    throw new Error(`no row for ${browserLine}`);
  }
  return row;
};

async function pause(ms: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

beforeEach(() => {
  useAuthStore.setState(pristine.auth, true);
  useListenersStore.setState(pristine.listeners, true);
  useStatusStore.setState(pristine.status, true);
  vi.spyOn(Date, 'now').mockReturnValue(NOW);
  useAuthStore.setState({
    mode: 'accounts',
    user: { id: 1, email: 'owner@example.com', role: 'admin', createdAtMs: NOW, twoFactorEnabled: false },
  });
  useStatusStore.setState({ status: STATUS });
  useListenersStore.setState({ listeners: LISTENERS });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('the count, for somebody not allowed to see who is listening', () => {
  beforeEach(() => {
    useListenersStore.setState({ listeners: null });
  });

  it('shows the polled count with nothing behind it', () => {
    render(<ListenersBadge />);
    expect(screen.getByText('3 listeners')).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('says listener in the singular for one', () => {
    useStatusStore.setState({ status: { ...STATUS, listeners: 1 } });
    render(<ListenersBadge />);
    expect(screen.getByText('1 listener')).toBeInTheDocument();
  });

  it('shows none before the first status arrives', () => {
    useStatusStore.setState({ status: null });
    render(<ListenersBadge />);
    expect(screen.getByText('0 listeners')).toBeInTheDocument();
  });
});

describe('the count, for somebody allowed to see who is listening', () => {
  it('counts the pushed list, which is current to the moment, over the polled count', () => {
    render(<ListenersBadge />);
    expect(trigger(4)).toBeInTheDocument();
  });

  it('is closed until asked', () => {
    render(<ListenersBadge />);
    expect(heading()).not.toBeInTheDocument();
  });
});

describe('opening the list', () => {
  it('opens when a mouse points at the count, without taking focus', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    await user.hover(trigger());

    expect(heading()).toBeInTheDocument();
    expect(screen.getByText('Updates as people connect and leave')).toBeInTheDocument();
    expect(screen.getByRole('dialog')).not.toContainElement(document.activeElement as HTMLElement);
  });

  it('closes shortly after the mouse moves away', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    await user.hover(trigger());
    await user.unhover(trigger());

    // A grace period, so the pointer can cross from the count into the list.
    expect(heading()).toBeInTheDocument();
    await pause(250);
    expect(heading()).not.toBeInTheDocument();
  });

  it('stays open when the mouse moves from the count into the list', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    await user.hover(trigger());

    fireEvent.pointerLeave(trigger(), { pointerType: 'mouse' });
    fireEvent.pointerEnter(screen.getByRole('dialog'), { pointerType: 'mouse' });
    await pause(250);

    expect(heading()).toBeInTheDocument();
  });

  it('closes shortly after the mouse leaves the list', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    await user.hover(trigger());
    fireEvent.pointerLeave(trigger(), { pointerType: 'mouse' });
    fireEvent.pointerEnter(screen.getByRole('dialog'), { pointerType: 'mouse' });
    fireEvent.pointerLeave(screen.getByRole('dialog'), { pointerType: 'mouse' });
    await pause(250);

    expect(heading()).not.toBeInTheDocument();
  });

  it('is not closed by a mouse click after hovering opened it', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    await user.hover(trigger());
    fireEvent(trigger(), new PointerEvent('click', { bubbles: true, cancelable: true, pointerType: 'mouse' }));

    expect(heading()).toBeInTheDocument();
  });

  it('opens and closes with a tap, since touch screens have no hover', () => {
    render(<ListenersBadge />);
    fireEvent.click(trigger());
    expect(heading()).toBeInTheDocument();
    fireEvent.click(trigger());
    expect(heading()).not.toBeInTheDocument();
  });

  it('opens with Enter and closes with Escape', async () => {
    const user = userEvent.setup();
    render(<ListenersBadge />);
    trigger().focus();
    await user.keyboard('{Enter}');
    expect(heading()).toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(heading()).not.toBeInTheDocument();
  });

  it('ignores a touch passing over the count', () => {
    render(<ListenersBadge />);
    fireEvent.pointerEnter(trigger(), { pointerType: 'touch' });
    expect(heading()).not.toBeInTheDocument();
  });
});

describe('the list', () => {
  function openList() {
    render(<ListenersBadge />);
    fireEvent.click(trigger());
    return screen.getByRole('dialog');
  }

  /** The people in the list, in order: the name on each group's first line. */
  const people = (dialog: HTMLElement) =>
    within(dialog)
      .getAllByRole('listitem')
      .filter((item) => item.parentElement?.parentElement === dialog)
      .map((item) => item.querySelector('.font-medium')?.textContent);

  it('lists each person once, the viewer first and marked you, then everybody else by arrival', () => {
    const dialog = openList();
    expect(people(dialog)).toEqual(['owner@example.com', 'Guest', 'kitchen@example.com']);
    const owner = within(dialog).getByText('owner@example.com').closest('li') as HTMLElement;
    expect(within(owner).getByText('you')).toBeInTheDocument();
    expect(within(dialog).getAllByText('you')).toHaveLength(1);
  });

  it('shows each account role, and none for a guest', () => {
    const dialog = openList();
    const owner = within(dialog).getByText('owner@example.com').closest('li') as HTMLElement;
    const kitchen = within(dialog).getByText('kitchen@example.com').closest('li') as HTMLElement;
    const guest = within(dialog).getByText('Guest').closest('li') as HTMLElement;
    expect(within(owner).getByText('Admin')).toBeInTheDocument();
    expect(within(kitchen).getByText('Listener')).toBeInTheDocument();
    expect(within(guest).queryByText(/Admin|Listener/)).not.toBeInTheDocument();
  });

  it('puts every tab of one account under it, oldest first', () => {
    const dialog = openList();
    const kitchen = within(dialog).getByText('kitchen@example.com').closest('li') as HTMLElement;
    const tabs = within(kitchen).getAllByRole('listitem');
    expect(tabs).toHaveLength(2);
    expect(tabs[0]).toHaveTextContent('Safari on iOS');
    expect(tabs[1]).toHaveTextContent('Unknown browser');
  });

  it('says what each tab is doing', () => {
    openList();
    expect(tabRow('Chrome on macOS')).toHaveTextContent('Live');
    expect(tabRow('Safari on iOS')).toHaveTextContent(`History from ${formatClock(EARLIER_TODAY)}`);
    expect(tabRow('Unknown browser')).toHaveTextContent('Not playing');
    expect(tabRow('Firefox on Windows')).toHaveTextContent('Paused');
  });

  it('colours each dot by what is heard: red live, amber history, grey paused, hollow idle', () => {
    openList();
    const dot = (browserLine: string) => tabRow(browserLine).querySelector('.rounded-full');
    expect(dot('Chrome on macOS')).toHaveClass('bg-red-500');
    expect(dot('Safari on iOS')).toHaveClass('bg-amber-500');
    expect(dot('Firefox on Windows')).toHaveClass('bg-muted-foreground/50');
    expect(dot('Unknown browser')).toHaveClass('border');
  });

  it('names the browser, device and network address of each tab', () => {
    openList();
    expect(screen.getByText('Chrome on macOS \u00b7 192.168.1.10')).toBeInTheDocument();
    expect(screen.getByText('Firefox on Windows \u00b7 192.168.1.30')).toBeInTheDocument();
  });

  it('says how long each tab has been connected', () => {
    openList();
    expect(tabRow('Chrome on macOS')).toHaveTextContent('30 min');
    expect(tabRow('Safari on iOS')).toHaveTextContent('5 min');
    expect(tabRow('Unknown browser')).toHaveTextContent('just now');
    expect(tabRow('Firefox on Windows')).toHaveTextContent('2 h');
  });

  it('gives the full browser description and connection time on pointing', () => {
    openList();
    expect(tabRow('Chrome on macOS')).toHaveAttribute(
      'title',
      `${MAC_CHROME}\nConnected ${formatDateTime(OWNER.connectedAtMs)}`,
    );
    expect(tabRow('Unknown browser')).toHaveAttribute(
      'title',
      `No browser description\nConnected ${formatDateTime(KITCHEN_TAB.connectedAtMs)}`,
    );
  });

  it('keeps a long email whole in its title while the line is cut to fit', () => {
    const email = 'a.very.long.address.for.the.office.laptop@example.com';
    useListenersStore.setState({ listeners: [listener({ id: 9, email, role: 'listener' })] });
    render(<ListenersBadge />);
    fireEvent.click(screen.getByRole('button', { name: '1 listener, show who is listening' }));
    expect(screen.getByText(email)).toHaveAttribute('title', email);
  });

  it('groups guests by network address', () => {
    useListenersStore.setState({
      listeners: [
        listener({ id: 1, address: '10.0.0.5' }),
        listener({ id: 2, address: '10.0.0.5', player: 'idle' }),
        listener({ id: 3, address: '10.0.0.6' }),
      ],
    });
    useAuthStore.setState({ mode: 'open', user: null });
    render(<ListenersBadge />);
    fireEvent.click(trigger(3));
    expect(screen.getAllByText('Guest')).toHaveLength(2);
  });

  it('says so when nobody is listening', () => {
    useListenersStore.setState({ listeners: [] });
    render(<ListenersBadge />);
    fireEvent.click(trigger(0));
    expect(screen.getByText('Nobody is listening.')).toBeInTheDocument();
  });

  it('updates in place while open as people arrive and leave', () => {
    openList();
    act(() => {
      useListenersStore.setState({ listeners: [OWNER, GUEST] });
    });
    expect(screen.getByText('Listening now')).toBeInTheDocument();
    expect(screen.queryByText('kitchen@example.com')).not.toBeInTheDocument();
    expect(trigger(2)).toBeInTheDocument();
  });

  it('refreshes how long each tab has been connected while it stays open', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    try {
      openList();
      expect(tabRow('Unknown browser')).toHaveTextContent('just now');

      vi.mocked(Date.now).mockReturnValue(NOW + 2 * 60_000);
      act(() => {
        vi.advanceTimersByTime(15_000);
      });
      expect(tabRow('Unknown browser')).toHaveTextContent('2 min');
    } finally {
      vi.useRealTimers();
    }
  });
});
