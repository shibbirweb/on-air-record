/**
 * The bookmark store: loading the set, adding, renaming and removing, with the saving flag and the error
 * each one leaves behind.
 *
 * The list is kept in timeline order locally so the timeline and the minimap can draw it without a
 * refetch, so the insertion order after `add` matters as much as the request that was made. Requests are
 * held open with a deferred promise where the test needs to see the saving flag while one is in flight.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Bookmark } from '@/api/types';

const server = vi.hoisted(() => ({
  bookmarks: [] as Bookmark[],
  calls: [] as unknown[][],
  failWith: null as Error | null,
  /** When set, the next request waits for this before answering. */
  gate: null as Promise<void> | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const step = async (name: string, args: unknown[]) => {
    server.calls.push([name, ...args]);
    if (server.gate) {
      await server.gate;
    }
    if (server.failWith) {
      throw server.failWith;
    }
  };
  return {
    ...actual,
    api: {
      bookmarks: async () => {
        await step('bookmarks', []);
        return server.bookmarks.map((item) => ({ ...item }));
      },
      createBookmark: async (timestampMs: number, label: string, note?: string | null) => {
        await step('createBookmark', [timestampMs, label, note]);
        return { id: 99, timestampMs, label, note: note ?? null, createdAtMs: 0 };
      },
      updateBookmark: async (id: number, patch: { label?: string }) => {
        await step('updateBookmark', [id, patch]);
        const found = server.bookmarks.find((item) => item.id === id);
        return { ...found, ...patch };
      },
      deleteBookmark: async (id: number) => {
        await step('deleteBookmark', [id]);
        return server.bookmarks.filter((item) => item.id !== id);
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useBookmarkStore } = await import('../useBookmarkStore');

function bookmark(id: number, timestampMs: number, label = `Mark ${id}`): Bookmark {
  return { id, timestampMs, label, note: null, createdAtMs: 0 };
}

function deferred() {
  let release = () => undefined as void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

beforeEach(() => {
  server.bookmarks = [bookmark(1, 1_000), bookmark(2, 3_000)];
  server.calls = [];
  server.failWith = null;
  server.gate = null;
  useBookmarkStore.setState({ bookmarks: [], saving: false, error: null });
});

describe('refresh', () => {
  it('loads the whole set', async () => {
    await useBookmarkStore.getState().refresh();
    expect(useBookmarkStore.getState().bookmarks.map((item) => item.id)).toEqual([1, 2]);
    expect(useBookmarkStore.getState().error).toBeNull();
  });

  it('keeps what it had and says why when loading fails', async () => {
    useBookmarkStore.setState({ bookmarks: [bookmark(5, 10)] });
    server.failWith = new ApiError('the database is locked', 'internal', 500);
    await useBookmarkStore.getState().refresh();

    expect(useBookmarkStore.getState().bookmarks.map((item) => item.id)).toEqual([5]);
    expect(useBookmarkStore.getState().error).toBe('the database is locked');
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.failWith = new Error('boom');
    await useBookmarkStore.getState().refresh();
    expect(useBookmarkStore.getState().error).toBe('could not load bookmarks');
  });

  it('clears an earlier error on success', async () => {
    useBookmarkStore.setState({ error: 'old problem' });
    await useBookmarkStore.getState().refresh();
    expect(useBookmarkStore.getState().error).toBeNull();
  });
});

describe('add', () => {
  beforeEach(async () => {
    await useBookmarkStore.getState().refresh();
  });

  it('creates the bookmark and returns it', async () => {
    const created = await useBookmarkStore.getState().add(2_000, 'Middle', 'a note');
    expect(server.calls.at(-1)).toEqual(['createBookmark', 2_000, 'Middle', 'a note']);
    expect(created).toMatchObject({ id: 99, timestampMs: 2_000, label: 'Middle', note: 'a note' });
  });

  it('slots the new bookmark into timeline order rather than appending it', async () => {
    await useBookmarkStore.getState().add(2_000, 'Middle');
    expect(useBookmarkStore.getState().bookmarks.map((item) => item.timestampMs)).toEqual([
      1_000,
      2_000,
      3_000,
    ]);
  });

  it('is saving while the request is in flight, and not after', async () => {
    const hold = deferred();
    server.gate = hold.promise;
    const pending = useBookmarkStore.getState().add(2_000, 'Middle');
    expect(useBookmarkStore.getState().saving).toBe(true);

    hold.release();
    await pending;
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('returns null, keeps the list and says why when refused', async () => {
    server.failWith = new ApiError('a label is required', 'bad_request', 400);
    const created = await useBookmarkStore.getState().add(2_000, '');

    expect(created).toBeNull();
    expect(useBookmarkStore.getState().bookmarks).toHaveLength(2);
    expect(useBookmarkStore.getState().error).toBe('a label is required');
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('uses its own words when adding fails for another reason', async () => {
    server.failWith = new Error('boom');
    await useBookmarkStore.getState().add(2_000, 'Middle');
    expect(useBookmarkStore.getState().error).toBe('could not add the bookmark');
  });

  it('clears an earlier error on success', async () => {
    useBookmarkStore.setState({ error: 'old problem' });
    await useBookmarkStore.getState().add(2_000, 'Middle');
    expect(useBookmarkStore.getState().error).toBeNull();
  });
});

describe('rename', () => {
  beforeEach(async () => {
    await useBookmarkStore.getState().refresh();
  });

  it('sends only the label and replaces that one bookmark in place', async () => {
    await useBookmarkStore.getState().rename(2, 'Outro');

    expect(server.calls.at(-1)).toEqual(['updateBookmark', 2, { label: 'Outro' }]);
    expect(useBookmarkStore.getState().bookmarks.map((item) => item.label)).toEqual(['Mark 1', 'Outro']);
    expect(useBookmarkStore.getState().error).toBeNull();
  });

  it('is saving while the request is in flight, and not after', async () => {
    const hold = deferred();
    server.gate = hold.promise;
    const pending = useBookmarkStore.getState().rename(2, 'Outro');
    expect(useBookmarkStore.getState().saving).toBe(true);

    hold.release();
    await pending;
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('keeps the old label and says why when refused', async () => {
    server.failWith = new ApiError('no such bookmark', 'not_found', 404);
    await useBookmarkStore.getState().rename(2, 'Outro');

    expect(useBookmarkStore.getState().bookmarks.map((item) => item.label)).toEqual(['Mark 1', 'Mark 2']);
    expect(useBookmarkStore.getState().error).toBe('no such bookmark');
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('uses its own words when renaming fails for another reason', async () => {
    server.failWith = new Error('boom');
    await useBookmarkStore.getState().rename(2, 'Outro');
    expect(useBookmarkStore.getState().error).toBe('could not rename the bookmark');
  });
});

describe('remove', () => {
  beforeEach(async () => {
    await useBookmarkStore.getState().refresh();
  });

  it('takes what remains from the server answer', async () => {
    await useBookmarkStore.getState().remove(1);

    expect(server.calls.at(-1)).toEqual(['deleteBookmark', 1]);
    expect(useBookmarkStore.getState().bookmarks.map((item) => item.id)).toEqual([2]);
    expect(useBookmarkStore.getState().error).toBeNull();
  });

  it('is saving while the request is in flight, and not after', async () => {
    const hold = deferred();
    server.gate = hold.promise;
    const pending = useBookmarkStore.getState().remove(1);
    expect(useBookmarkStore.getState().saving).toBe(true);

    hold.release();
    await pending;
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('keeps the list and says why when refused', async () => {
    server.failWith = new ApiError('sign in as an admin', 'forbidden', 403);
    await useBookmarkStore.getState().remove(1);

    expect(useBookmarkStore.getState().bookmarks).toHaveLength(2);
    expect(useBookmarkStore.getState().error).toBe('sign in as an admin');
    expect(useBookmarkStore.getState().saving).toBe(false);
  });

  it('uses its own words when removing fails for another reason', async () => {
    server.failWith = new Error('boom');
    await useBookmarkStore.getState().remove(1);
    expect(useBookmarkStore.getState().error).toBe('could not remove the bookmark');
  });
});

describe('clearError', () => {
  it('dismisses the error', () => {
    useBookmarkStore.setState({ error: 'something' });
    useBookmarkStore.getState().clearError();
    expect(useBookmarkStore.getState().error).toBeNull();
  });
});
