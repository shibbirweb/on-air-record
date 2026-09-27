// @vitest-environment jsdom
/**
 * `useListenerCount`, the hook the header badge reads. `useListenersStore.test.ts` covers the pure
 * `listenerCount` rule; this proves the hook wires it to the two stores and follows both as they change,
 * preferring the pushed list and falling back to the polled status when the list is taken away.
 */

import '@/test/dom';

import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import type { ListenerView, ServiceStatus } from '@/api/types';
import { useStatusStore } from '@/store/useStatusStore';

import { useListenerCount, useListenersStore } from '../useListenersStore';

function listener(id: number): ListenerView {
  return {
    id,
    email: null,
    role: null,
    address: '192.168.1.24',
    userAgent: null,
    connectedAtMs: 0,
    activity: 'live',
    fromMs: null,
    player: 'playing',
  };
}

function polled(listeners: number): ServiceStatus {
  return { listeners } as ServiceStatus;
}

beforeEach(() => {
  useListenersStore.setState({ listeners: null });
  useStatusStore.setState({ status: null });
});

describe('useListenerCount', () => {
  it('is zero before anything arrives', () => {
    const { result } = renderHook(() => useListenerCount());
    expect(result.current).toBe(0);
  });

  it('follows the polled status, then the pushed list once it arrives, then the poll again', () => {
    const { result } = renderHook(() => useListenerCount());

    act(() => {
      useStatusStore.setState({ status: polled(4) });
    });
    expect(result.current).toBe(4);

    act(() => {
      useListenersStore.getState().setListeners([listener(1), listener(2)]);
    });
    expect(result.current).toBe(2);

    act(() => {
      useListenersStore.getState().clear();
    });
    expect(result.current).toBe(4);
  });
});
