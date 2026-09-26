/**
 * Who is connected right now, pushed over the stream socket.
 *
 * Only a session that may see the list receives it (an admin, or anybody on an open recorder), so `null`
 * means "not yours to see" as much as "not arrived yet". Pushed rather than polled because the whole point
 * is to watch people come and go as it happens, and the server already has a socket open to every admin.
 */

import { create } from 'zustand';

import type { ListenerView } from '@/api/types';
import { useStatusStore } from '@/store/useStatusStore';

type ListenersState = {
  listeners: ListenerView[] | null;
  setListeners: (listeners: ListenerView[]) => void;
  /** The list was taken away, or the socket that carried it closed and it can no longer be trusted. */
  clear: () => void;
};

export const useListenersStore = create<ListenersState>((set) => ({
  listeners: null,
  setListeners: (listeners) => set({ listeners }),
  clear: () => set({ listeners: null }),
}));

/**
 * The number of listeners to show. The pushed list when there is one, because it is current to the moment;
 * the polled status otherwise, which is what a listener account sees and is at most a second old.
 */
export function listenerCount(pushed: ListenerView[] | null, polled: number | null | undefined): number {
  return pushed?.length ?? polled ?? 0;
}

export function useListenerCount(): number {
  const pushed = useListenersStore((state) => state.listeners);
  const polled = useStatusStore((state) => state.status?.listeners);
  return listenerCount(pushed, polled);
}
