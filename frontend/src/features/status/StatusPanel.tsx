/** Recorder health at a glance: state, device, listeners and the input meter. */

import { Circle, Loader2, Square, TriangleAlert } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { LevelMeter } from '@/features/broadcast/LevelMeter';
import { formatDuration } from '@/lib/format';
import { useCanAdminister } from '@/store/useAuthStore';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useStatusStore } from '@/store/useStatusStore';

export function StatusPanel() {
  const status = useStatusStore((state) => state.status);
  const reachable = useStatusStore((state) => state.reachable);
  const busy = useStatusStore((state) => state.busy);
  const error = useStatusStore((state) => state.error);
  const startCapture = useStatusStore((state) => state.startCapture);
  const stopCapture = useStatusStore((state) => state.stopCapture);
  // A listener sees the recorder's state but cannot start or stop it.
  const mayAdminister = useCanAdminister();

  const levels = useConnectionStore((state) => state.levels);
  const connected = useConnectionStore((state) => state.connected);

  const capture = status?.capture;
  const recording = capture?.state === 'recording';
  const starting = capture?.state === 'starting';
  // Taken from the server clock rather than the browser's, so a machine with a skewed clock does not
  // report a session that started in the future.
  const elapsedMs =
    capture?.startedAtMs && status ? Math.max(status.serverTimeMs - capture.startedAtMs, 0) : 0;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {recording ? (
            <Badge variant="live" className="gap-1.5">
              <Circle className="size-2 animate-pulse fill-current" />
              Recording
            </Badge>
          ) : starting ? (
            <Badge variant="secondary" className="gap-1.5">
              <Loader2 className="size-3 animate-spin" />
              Starting
            </Badge>
          ) : capture?.state === 'error' ? (
            <Badge variant="destructive" className="gap-1.5">
              <TriangleAlert className="size-3" />
              Error
            </Badge>
          ) : (
            <Badge variant="outline">Idle</Badge>
          )}

          {recording && <span className="text-muted-foreground text-xs tabular">{formatDuration(elapsedMs)}</span>}
        </div>

        {mayAdminister && (
          <Button
            size="sm"
            variant={recording ? 'outline' : 'default'}
            disabled={busy || !reachable}
            onClick={() => void (recording ? stopCapture() : startCapture())}
          >
            {recording ? <Square /> : <Circle />}
            {recording ? 'Stop' : 'Record'}
          </Button>
        )}
      </div>

      <LevelMeter rms={levels.rms} peak={levels.peak} active={recording && connected} />

      <Separator />

      <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
        <dt className="text-muted-foreground">Format</dt>
        <dd className="tabular">
          {capture && capture.sampleRate > 0
            ? `${(capture.sampleRate / 1000).toFixed(1)} kHz, ${capture.channels === 1 ? 'mono' : `${capture.channels} ch`}`
            : 'not started'}
        </dd>

        <dt className="text-muted-foreground">Listeners</dt>
        <dd className="tabular">{status?.listeners ?? 0}</dd>

        <dt className="text-muted-foreground">Session</dt>
        <dd className="tabular">{capture?.sessionId ?? 'none'}</dd>

        <dt className="text-muted-foreground">Dropped frames</dt>
        <dd className="tabular">{capture?.droppedFrames ?? 0}</dd>

        <dt className="text-muted-foreground">Broadcast link</dt>
        <dd>{connected ? 'connected' : 'reconnecting'}</dd>
      </dl>

      {capture?.error && <p className="text-destructive text-xs">{capture.error}</p>}
      {!reachable && error && <p className="text-destructive text-xs">{error}</p>}
    </div>
  );
}
