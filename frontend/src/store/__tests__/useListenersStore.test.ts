import { beforeEach, describe, expect, it } from 'vitest';

import type { ListenerView } from '@/api/types';

import { listenerCount, useListenersStore } from '../useListenersStore';

function listener(id: number): ListenerView {
  return {
    id,
    email: null,
    role: null,
    address: '192.168.1.24',
    userAgent: null,
    connectedAtMs: 1_757_034_000_000,
    activity: 'live',
    fromMs: null,
    player: 'playing',
  };
}

describe('useListenersStore', () => {
  beforeEach(() => {
    useListenersStore.setState({ listeners: null });
  });

  it('starts with no list, which reads as not yours to see', () => {
    expect(useListenersStore.getState().listeners).toBeNull();
  });

  it('replaces the list wholesale on every push', () => {
    useListenersStore.getState().setListeners([listener(1), listener(2)]);
    useListenersStore.getState().setListeners([listener(2)]);
    expect(useListenersStore.getState().listeners?.map((entry) => entry.id)).toEqual([2]);
  });

  it('drops the list when it is hidden or the socket closes', () => {
    useListenersStore.getState().setListeners([listener(1)]);
    useListenersStore.getState().clear();
    expect(useListenersStore.getState().listeners).toBeNull();
  });
});

describe('listenerCount', () => {
  it('prefers the pushed list, which is current to the moment', () => {
    expect(listenerCount([listener(1), listener(2), listener(3)], 1)).toBe(3);
  });

  it('trusts an empty pushed list over a stale poll', () => {
    expect(listenerCount([], 2)).toBe(0);
  });

  it('falls back to the polled status without a list', () => {
    expect(listenerCount(null, 4)).toBe(4);
  });

  it('is zero before anything has arrived', () => {
    expect(listenerCount(null, undefined)).toBe(0);
    expect(listenerCount(null, null)).toBe(0);
  });
});
