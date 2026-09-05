/**
 * Settings, on their own page.
 *
 * Grouped by the question each answers rather than by which part of the code owns them: what the audio
 * sounds like, how much history is kept and what it costs, and where the files land.
 */

import { useEffect } from 'react';

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { RecordingLocation } from '@/features/settings/RecordingLocation';
import { RecordingQuality } from '@/features/settings/RecordingQuality';
import { SettingsActionBar } from '@/features/settings/SettingsActionBar';
import { RetentionSettings } from '@/features/settings/RetentionSettings';
import { SettingsPanel } from '@/features/settings/SettingsPanel';
import { usePolling } from '@/hooks/usePolling';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStorageStore } from '@/store/useStorageStore';

/** The projection needs the recording rate and current usage, which only change slowly. */
const STORAGE_POLL_MS = 5000;

export function SettingsPage() {
  const refreshSettings = useSettingsStore((state) => state.refresh);
  const refreshStorage = useStorageStore((state) => state.refresh);

  useEffect(() => {
    // Only pull from the server when there is nothing pending, or a refresh would overwrite edits that
    // have not been saved yet.
    if (Object.keys(useSettingsStore.getState().draft).length === 0) {
      void refreshSettings();
    }
  }, [refreshSettings]);

  usePolling(refreshStorage, STORAGE_POLL_MS);

  return (
    <main className="mx-auto max-w-3xl space-y-4 px-4 py-6">
      <div className="space-y-1">
        <h2 className="text-xl font-semibold">Settings</h2>
        <p className="text-muted-foreground text-sm">
          Edits are held until you save them. Nothing reaches the recorder before that.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>History and storage</CardTitle>
          <CardDescription>
            The bit rate recordings are kept at, how far back the timeline reaches, and how much disk
            that needs.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <RecordingQuality />
          <RetentionSettings />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Recording location</CardTitle>
          <CardDescription>Where segment files are written on the host machine.</CardDescription>
        </CardHeader>
        <CardContent>
          <RecordingLocation />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Audio</CardTitle>
          <CardDescription>Input gain, segment length, and start up behaviour.</CardDescription>
        </CardHeader>
        <CardContent>
          <SettingsPanel />
        </CardContent>
      </Card>

      <SettingsActionBar />
    </main>
  );
}
