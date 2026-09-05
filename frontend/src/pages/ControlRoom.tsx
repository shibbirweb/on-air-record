/**
 * The control room: one screen with everything needed to run the station.
 *
 * Layout follows the order of attention. The broadcast and the timeline take the wide column because they
 * are what a listener actually uses, while the recorder, the microphone and the settings sit in a rail
 * that is read occasionally and touched rarely.
 */

import { useEffect, useRef } from 'react';

import { useAppContext } from '@/components/AppShell';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { LiveWaveform } from '@/features/broadcast/LiveWaveform';
import { TransportBar } from '@/features/broadcast/TransportBar';
import { DeviceSelector } from '@/features/devices/DeviceSelector';
import { StatusPanel } from '@/features/status/StatusPanel';
import { StoragePanel } from '@/features/status/StoragePanel';
import { TimelineMinimap } from '@/features/timeline/TimelineMinimap';
import { TimelineScrubber } from '@/features/timeline/TimelineScrubber';
import { TimelineToolbar } from '@/features/timeline/TimelineToolbar';
import { usePolling } from '@/hooks/usePolling';
import { useStorageStore } from '@/store/useStorageStore';
import { useBookmarkStore } from '@/store/useBookmarkStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

const TIMELINE_POLL_MS = 2000;
const STORAGE_POLL_MS = 10_000;
/** The set of recorded days only changes at midnight or when the janitor prunes, so poll it rarely. */
const DAYS_POLL_MS = 30_000;
/** The minimap covers a whole day, so it only needs to notice the live edge creeping along. */
const MINIMAP_POLL_MS = 10_000;
const BOOKMARK_POLL_MS = 20_000;

/** Wait for the window to settle before refetching the envelope, so a drag makes one request, not fifty. */
const PEAKS_DEBOUNCE_MS = 180;

export function ControlRoom() {
  const { engine, playheadMs } = useAppContext();

  const refreshRange = useTimelineStore((state) => state.refreshRange);
  const refreshPeaks = useTimelineStore((state) => state.refreshPeaks);
  const refreshDays = useTimelineStore((state) => state.refreshDays);
  const refreshDayPeaks = useTimelineStore((state) => state.refreshDayPeaks);
  const refreshStorage = useStorageStore((state) => state.refresh);
  const refreshBookmarks = useBookmarkStore((state) => state.refresh);

  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);

  const playing = useTransportStore((state) => state.playing);
  const mode = useTransportStore((state) => state.mode);

  usePolling(refreshRange, TIMELINE_POLL_MS);
  usePolling(refreshStorage, STORAGE_POLL_MS);
  usePolling(refreshDays, DAYS_POLL_MS);
  usePolling(refreshDayPeaks, MINIMAP_POLL_MS);
  // Bookmarks only change when somebody makes one, or when the janitor prunes with the audio.
  usePolling(refreshBookmarks, BOOKMARK_POLL_MS);

  const minimapStartMs = useTimelineStore((state) => state.minimapWindow().startMs);
  useEffect(() => {
    void refreshDayPeaks();
  }, [minimapStartMs, refreshDayPeaks]);

  const peaksTimer = useRef<number | null>(null);
  useEffect(() => {
    if (peaksTimer.current !== null) {
      window.clearTimeout(peaksTimer.current);
    }
    peaksTimer.current = window.setTimeout(() => void refreshPeaks(), PEAKS_DEBOUNCE_MS);

    return () => {
      if (peaksTimer.current !== null) {
        window.clearTimeout(peaksTimer.current);
      }
    };
  }, [windowStartMs, spanMs, refreshPeaks]);

    return (
    <>
      <main className="mx-auto grid max-w-[1600px] gap-4 px-4 py-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Broadcast</CardTitle>
              <CardDescription>
                {playing
                  ? mode === 'live'
                    ? 'Playing the live feed from the studio microphone.'
                    : 'Playing back recorded audio from the timeline.'
                  : 'Press play to start listening. Browsers only allow audio after a click.'}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="bg-muted/40 rounded-lg border p-2">
                <LiveWaveform engine={engine} />
              </div>
              <TransportBar getPlayheadMs={playheadMs} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Timeline</CardTitle>
              <CardDescription>
                Click anywhere to play from that moment, drag to pan, scroll to zoom. Shaded bands are the
                stretches that were recorded. The bar underneath is the whole day, with the visible window
                marked on it, and dragging that window moves the timeline. Bookmarks appear as flags on
                both.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <TimelineToolbar getPlayheadMs={playheadMs} />
              <TimelineScrubber getPlayheadMs={playheadMs} />
              <TimelineMinimap getPlayheadMs={playheadMs} />
            </CardContent>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Recorder</CardTitle>
            </CardHeader>
            <CardContent>
              <StatusPanel />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Source</CardTitle>
            </CardHeader>
            <CardContent>
              <DeviceSelector />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Storage</CardTitle>
            </CardHeader>
            <CardContent>
              <StoragePanel />
            </CardContent>
          </Card>
        </div>
      </main>
    </>
  );
}
