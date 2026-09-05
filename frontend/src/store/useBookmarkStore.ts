/**
 * Named moments on the timeline.
 *
 * Held in full rather than fetched per window: bookmarks are deliberate human acts, so there are tens of
 * them where there are thousands of segments, and having the whole set on hand means the timeline and the
 * minimap can both draw without a request every time the view moves.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { Bookmark } from '@/api/types';

type BookmarkState = {
  /** Ordered by position on the timeline, oldest first, as the server returns them. */
  bookmarks: Bookmark[];
  saving: boolean;
  error: string | null;

  refresh: () => Promise<void>;
  add: (timestampMs: number, label: string, note?: string | null) => Promise<Bookmark | null>;
  rename: (id: number, label: string) => Promise<void>;
  remove: (id: number) => Promise<void>;
  clearError: () => void;
};

export const useBookmarkStore = create<BookmarkState>((set) => ({
  bookmarks: [],
  saving: false,
  error: null,

  clearError: () => set({ error: null }),

  refresh: async () => {
    try {
      set({ bookmarks: await api.bookmarks(), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load bookmarks' });
    }
  },

  add: async (timestampMs, label, note) => {
    set({ saving: true });
    try {
      const created = await api.createBookmark(timestampMs, label, note);
      // Inserted in timeline order rather than appended, so the list matches what the server would
      // return next without waiting for a refetch.
      set((state) => ({
        bookmarks: [...state.bookmarks, created].sort((a, b) => a.timestampMs - b.timestampMs),
        error: null,
      }));
      return created;
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not add the bookmark' });
      return null;
    } finally {
      set({ saving: false });
    }
  },

  rename: async (id, label) => {
    set({ saving: true });
    try {
      const updated = await api.updateBookmark(id, { label });
      set((state) => ({
        bookmarks: state.bookmarks.map((item) => (item.id === id ? updated : item)),
        error: null,
      }));
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not rename the bookmark' });
    } finally {
      set({ saving: false });
    }
  },

  remove: async (id) => {
    set({ saving: true });
    try {
      // The server hands back what remains, so one request both deletes and refreshes.
      set({ bookmarks: await api.deleteBookmark(id), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not remove the bookmark' });
    } finally {
      set({ saving: false });
    }
  },
}));
