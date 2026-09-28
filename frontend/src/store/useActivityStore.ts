/**
 * The activity log as the Activity page shows it: a page of entries at a time, newest first, under the
 * filters chosen.
 *
 * Paging goes by the last entry shown rather than by page number, so an entry logged while somebody reads
 * never shifts what "load more" brings. Each request is numbered and only the latest may write its answer,
 * because a filter typed quickly sends several and they can come back in any order.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { ActivityEntry, ActivityFilter, ActivityGroup } from '@/api/types';

/** Entries asked for at a time. A full page back means there may be more. */
export const ACTIVITY_PAGE = 50;

type Filter = {
  /** Only what this account did. Empty for everybody. */
  email: string;
  group: ActivityGroup | null;
};

type ActivityState = {
  entries: ActivityEntry[];
  filter: Filter;
  /** Whether the service has answered once, so "nothing logged" is never shown before it is known. */
  loaded: boolean;
  loading: boolean;
  loadingMore: boolean;
  hasMore: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  loadMore: () => Promise<void>;
  setFilter: (change: Partial<Filter>) => Promise<void>;
};

const describe = (cause: unknown) =>
  cause instanceof ApiError ? cause.message : 'could not reach the recorder';

function query(filter: Filter, beforeId?: number): ActivityFilter {
  const asked: ActivityFilter = { limit: ACTIVITY_PAGE };
  const email = filter.email.trim();
  if (email) {
    asked.email = email;
  }
  if (filter.group) {
    asked.group = filter.group;
  }
  if (beforeId !== undefined) {
    asked.beforeId = beforeId;
  }
  return asked;
}

let latest = 0;

export const useActivityStore = create<ActivityState>((set, get) => ({
  entries: [],
  filter: { email: '', group: null },
  loaded: false,
  loading: false,
  loadingMore: false,
  hasMore: false,
  error: null,

  refresh: async () => {
    latest += 1;
    const ticket = latest;
    set({ loading: true });
    try {
      const { entries } = await api.activity(query(get().filter));
      if (ticket === latest) {
        set({ entries, loaded: true, hasMore: entries.length === ACTIVITY_PAGE, error: null });
      }
    } catch (cause) {
      if (ticket === latest) {
        set({ error: describe(cause) });
      }
    } finally {
      if (ticket === latest) {
        set({ loading: false });
      }
    }
  },

  loadMore: async () => {
    const { entries, hasMore, loadingMore, filter } = get();
    const last = entries[entries.length - 1];
    if (!hasMore || loadingMore || !last) {
      return;
    }
    const ticket = latest;
    set({ loadingMore: true });
    try {
      const page = await api.activity(query(filter, last.id));
      // A filter changed while this page was on its way: it belongs to a list no longer shown.
      if (ticket === latest) {
        set({
          entries: [...get().entries, ...page.entries],
          hasMore: page.entries.length === ACTIVITY_PAGE,
          error: null,
        });
      }
    } catch (cause) {
      if (ticket === latest) {
        set({ error: describe(cause) });
      }
    } finally {
      set({ loadingMore: false });
    }
  },

  setFilter: async (change) => {
    set({ filter: { ...get().filter, ...change } });
    await get().refresh();
  },
}));
