/** Disk usage and recording sessions, shown in the status column. */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { RecordingSession, Storage } from '@/api/types';

type StorageState = {
  storage: Storage | null;
  sessions: RecordingSession[];
  error: string | null;
  refresh: () => Promise<void>;
};

export const useStorageStore = create<StorageState>((set) => ({
  storage: null,
  sessions: [],
  error: null,

  refresh: async () => {
    try {
      const [storage, sessions] = await Promise.all([api.storage(), api.sessions()]);
      set({ storage, sessions, error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load storage usage' });
    }
  },
}));
