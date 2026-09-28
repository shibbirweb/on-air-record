/**
 * The activity page's state: a page of entries at a time, newest first, under the filters chosen. The
 * server pages by the last entry seen rather than by page number, so an entry arriving while somebody
 * reads never shifts what "load more" returns.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { api, ApiError } from '@/api/client';
import type { ActivityEntry, ActivityFilter } from '@/api/types';

import { ACTIVITY_PAGE, useActivityStore } from '../useActivityStore';

const KITCHEN = 'kitchen@example.com';

const initial = useActivityStore.getState();

function entry(id: number): ActivityEntry {
  return {
    id,
    atMs: 1_790_000_000_000 + id,
    actor: { kind: 'guest' },
    address: null,
    userAgent: null,
    event: { kind: 'signed_out' },
  };
}

/** A full page of entries counting down from `top`. */
function page(top: number, length = ACTIVITY_PAGE): ActivityEntry[] {
  return Array.from({ length }, (_, index) => entry(top - index));
}

let asked: ActivityFilter[] = [];

beforeEach(() => {
  useActivityStore.setState(initial, true);
  asked = [];
});

afterEach(() => {
  vi.restoreAllMocks();
});

function answer(...pages: ActivityEntry[][]) {
  vi.spyOn(api, 'activity').mockImplementation(async (filter) => {
    asked.push(filter);
    return { entries: pages.shift() ?? [] };
  });
}

describe('refresh', () => {
  it('reads the newest page under no filter at first', async () => {
    answer(page(100, 3));
    await useActivityStore.getState().refresh();
    expect(asked).toEqual([{ limit: ACTIVITY_PAGE }]);
    expect(useActivityStore.getState()).toMatchObject({
      loaded: true,
      loading: false,
      hasMore: false,
      error: null,
    });
    expect(useActivityStore.getState().entries.map((item) => item.id)).toEqual([100, 99, 98]);
  });

  it('expects more when the page came back full', async () => {
    answer(page(500));
    await useActivityStore.getState().refresh();
    expect(useActivityStore.getState().hasMore).toBe(true);
  });

  it('says what went wrong and keeps what it had', async () => {
    answer(page(100, 2));
    await useActivityStore.getState().refresh();
    vi.restoreAllMocks();
    vi.spyOn(api, 'activity').mockRejectedValue(new ApiError('admins only', 'forbidden', 403));
    await useActivityStore.getState().refresh();
    expect(useActivityStore.getState().error).toBe('admins only');
    expect(useActivityStore.getState().entries).toHaveLength(2);
    expect(useActivityStore.getState().loading).toBe(false);
  });

  it('describes a failure that never reached the recorder', async () => {
    vi.spyOn(api, 'activity').mockRejectedValue(new TypeError('Failed to fetch'));
    await useActivityStore.getState().refresh();
    expect(useActivityStore.getState().error).toBe('could not reach the recorder');
  });
});

describe('filters', () => {
  it('reads again from the top with the filter applied', async () => {
    answer(page(100, 2), page(40, 1));
    await useActivityStore.getState().refresh();
    await useActivityStore.getState().setFilter({ email: KITCHEN });
    expect(asked[1]).toEqual({ limit: ACTIVITY_PAGE, email: KITCHEN });
    expect(useActivityStore.getState().entries.map((item) => item.id)).toEqual([40]);
  });

  it('combines filters and leaves out the ones cleared', async () => {
    answer([], [], []);
    await useActivityStore.getState().setFilter({ email: `  ${KITCHEN} ` });
    await useActivityStore.getState().setFilter({ group: 'listening' });
    expect(asked[1]).toEqual({ limit: ACTIVITY_PAGE, email: KITCHEN, group: 'listening' });
    await useActivityStore.getState().setFilter({ email: '', group: null });
    expect(asked[2]).toEqual({ limit: ACTIVITY_PAGE });
  });

  /** Somebody typing a name changes the filter faster than the server answers. Only the latest counts. */
  it('never lets an older answer overwrite a newer filter', async () => {
    const replies: ((entries: ActivityEntry[]) => void)[] = [];
    vi.spyOn(api, 'activity').mockImplementation(
      () =>
        new Promise((resolve) => {
          replies.push((entries) => resolve({ entries }));
        }),
    );
    const first = useActivityStore.getState().setFilter({ email: 'k' });
    const second = useActivityStore.getState().setFilter({ email: KITCHEN });
    replies[1]?.([entry(7)]);
    await second;
    replies[0]?.([entry(1), entry(2)]);
    await first;
    expect(useActivityStore.getState().entries.map((item) => item.id)).toEqual([7]);
    expect(useActivityStore.getState().loading).toBe(false);
  });
});

describe('loadMore', () => {
  it('asks for what comes after the last entry shown, under the same filter, and adds it', async () => {
    answer(page(500), page(500 - ACTIVITY_PAGE, 2));
    await useActivityStore.getState().setFilter({ group: 'access' });
    await useActivityStore.getState().loadMore();
    const lastShown = 500 - ACTIVITY_PAGE + 1;
    expect(asked[1]).toEqual({ limit: ACTIVITY_PAGE, group: 'access', beforeId: lastShown });
    const ids = useActivityStore.getState().entries.map((item) => item.id);
    expect(ids).toHaveLength(ACTIVITY_PAGE + 2);
    expect(ids.slice(-2)).toEqual([500 - ACTIVITY_PAGE, 499 - ACTIVITY_PAGE]);
    expect(useActivityStore.getState().hasMore).toBe(false);
  });

  it('does nothing when there is no more to load, or while a page is already on its way', async () => {
    answer(page(100, 2));
    await useActivityStore.getState().refresh();
    await useActivityStore.getState().loadMore();
    expect(asked).toHaveLength(1);

    useActivityStore.setState({ hasMore: true, loadingMore: true });
    await useActivityStore.getState().loadMore();
    expect(asked).toHaveLength(1);
  });
});

