/**
 * The control room: one screen with everything needed to run the station.
 *
 * Layout follows the order of attention. The broadcast and the timeline take the wide column because they
 * are what a listener actually uses, while the recorder, the microphone and the settings sit in a rail
 * that is read occasionally and touched rarely.
 */

import { Moon, Radio, Sun, Wifi, WifiOff } from 'lucide-react';
import { useEffect, useRef } from 'react';

import { OnAirSign } from '@/components/OnAirSign';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { LiveWaveform } from '@/features/broadcast/LiveWaveform';
import { TransportBar } from '@/features/broadcast/TransportBar';
import { DeviceSelector } from '@/features/devices/DeviceSelector';
import { SettingsPanel } from '@/features/settings/SettingsPanel';
import { StatusPanel } from '@/features/status/StatusPanel';
import { StoragePanel } from '@/features/status/StoragePanel';
import { TimelineScrubber } from '@/features/timeline/TimelineScrubber';
import { TimelineToolbar } from '@/features/timeline/TimelineToolbar';
import { usePolling } from '@/hooks/usePolling';
import { useStreamEngine } from '@/hooks/useStreamEngine';
import { useTheme } from '@/hooks/useTheme';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useStorageStore } from '@/store/useStorageStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

const STATUS_POLL_MS = 1000;
const TIMELINE_POLL_MS = 2000;
const STORAGE_POLL_MS = 10_000;
/** The set of recorded days only changes at midnight or when the janitor prunes, so poll it rarely. */
const DAYS_POLL_MS = 30_000;

/** Wait for the window to settle before refetching the envelope, so a drag makes one request, not fifty. */
const PEAKS_DEBOUNCE_MS = 180;

export function ControlRoom() {
  const { engine, playheadMs } = useStreamEngine();
  const { theme, toggleTheme } = useTheme();

  const refreshStatus = useStatusStore((state) => state.refresh);
  const refreshRange = useTimelineStore((state) => state.refreshRange);
  const refreshPeaks = useTimelineStore((state) => state.refreshPeaks);
  const refreshDays = useTimelineStore((state) => state.refreshDays);
  const refreshStorage = useStorageStore((state) => state.refresh);

  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);

  const connected = useConnectionStore((state) => state.connected);
  const capturing = useStatusStore((state) => state.status?.capture.state === 'recording');
  const listeners = useStatusStore((state) => state.status?.listeners ?? 0);
  const playing = useTransportStore((state) => state.playing);
  const mode = useTransportStore((state) => state.mode);

  usePolling(refreshStatus, STATUS_POLL_MS);
  usePolling(refreshRange, TIMELINE_POLL_MS);
  usePolling(refreshStorage, STORAGE_POLL_MS);
  usePolling(refreshDays, DAYS_POLL_MS);

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

  const onAir = capturing && playing && mode === 'live';

  return (
    <div className="bg-background min-h-screen">
      <header className="bg-background/85 sticky top-0 z-20 border-b backdrop-blur">
        <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-3 px-4 py-3">
          <div className="flex items-center gap-2.5">
            <div className="bg-primary text-primary-foreground grid size-9 place-items-center rounded-lg">
              <Radio className="size-5" />
            </div>
            <div className="leading-tight">
              <h1 className="text-base font-semibold">On Air Record</h1>
              <p className="text-muted-foreground text-xs">Local network audio broadcast and DVR</p>
            </div>
          </div>

          <OnAirSign live={onAir} className="ml-2" />

          <div className="ml-auto flex items-center gap-2">
            <Badge variant={connected ? 'secondary' : 'destructive'} className="gap-1.5 font-normal">
              {connected ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}
              {connected ? 'Stream connected' : 'Stream offline'}
            </Badge>
            <Badge variant="outline" className="font-normal">
              {listeners} {listeners === 1 ? 'listener' : 'listeners'}
            </Badge>
            <Button
              size="icon-sm"
              variant="ghost"
              onClick={toggleTheme}
              aria-label="Toggle colour theme"
            >
              {theme === 'dark' ? <Sun /> : <Moon />}
            </Button>
          </div>
        </div>
      </header>

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
                stretches that were recorded.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <TimelineToolbar />
              <TimelineScrubber getPlayheadMs={playheadMs} />
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
            <CardContent>
              <Tabs defaultValue="storage">
                <TabsList className="w-full">
                  <TabsTrigger value="storage">Storage</TabsTrigger>
                  <TabsTrigger value="settings">Settings</TabsTrigger>
                </TabsList>
                <TabsContent value="storage" className="pt-4">
                  <StoragePanel />
                </TabsContent>
                <TabsContent value="settings" className="pt-4">
                  <SettingsPanel />
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>
        </div>
      </main>
    </div>
  );
}
