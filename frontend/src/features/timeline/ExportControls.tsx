/**
 * Downloading a span of the recording as a WAV file.
 *
 * The range defaults to the window already on screen, because framing a span is exactly what the timeline
 * is for and asking someone to type two timestamps they have already scrolled to would be perverse.
 *
 * The plan is fetched before the download is offered, so the size, the format and any refusal are all
 * visible while the range can still be adjusted, rather than arriving as a failed download.
 */

import { AlertTriangle, Download, FileAudio, Loader2 } from 'lucide-react';
import { useState } from 'react';

import { api, ApiError } from '@/api/client';
import type { ExportPlan } from '@/api/types';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { formatBytes, formatClock, formatDateTime, formatDuration } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';

/** Quick spans measured back from the live edge, for the common "grab what just happened" case. */
const RECENT = [
  { label: 'Last 1m', ms: 60_000 },
  { label: 'Last 5m', ms: 5 * 60_000 },
  { label: 'Last 15m', ms: 15 * 60_000 },
] as const;

export function ExportControls() {
  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);
  const liveEdgeMs = useTimelineStore((state) => state.range?.liveEdgeMs ?? null);

  const [open, setOpen] = useState(false);
  const [range, setRange] = useState<{ fromMs: number; toMs: number } | null>(null);
  const [plan, setPlan] = useState<ExportPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = async (fromMs: number, toMs: number) => {
    setRange({ fromMs, toMs });
    setPlan(null);
    setError(null);
    setLoading(true);

    try {
      setPlan(await api.exportPlan(fromMs, toMs));
    } catch (cause) {
      setError(cause instanceof ApiError ? cause.message : 'could not work out the export');
    } finally {
      setLoading(false);
    }
  };

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (next) {
      // The visible window, snapshotted on open so it cannot shift under the dialogue while following
      // live scrolls the timeline along.
      void load(windowStartMs, windowStartMs + spanMs);
    }
  };

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button size="icon-sm" variant="outline" aria-label="Export audio">
          <Download />
        </Button>
      </PopoverTrigger>

      <PopoverContent align="start" className="w-80 p-0">
        <div className="space-y-1 px-3 py-2">
          <p className="text-sm font-medium">Export as WAV</p>
          <p className="text-muted-foreground text-xs">
            {range === null
              ? ''
              : `${formatDateTime(range.fromMs)} to ${formatClock(range.toMs)}`}
          </p>
        </div>

        <Separator />

        <div className="space-y-3 p-3">
          <div className="flex flex-wrap gap-1.5">
            <Button
              size="sm"
              variant="secondary"
              className="h-7"
              onClick={() => void load(windowStartMs, windowStartMs + spanMs)}
            >
              Visible window
            </Button>
            {RECENT.map((option) => (
              <Button
                key={option.ms}
                size="sm"
                variant="outline"
                className="h-7"
                disabled={liveEdgeMs === null}
                onClick={() => {
                  if (liveEdgeMs !== null) {
                    void load(liveEdgeMs - option.ms, liveEdgeMs);
                  }
                }}
              >
                {option.label}
              </Button>
            ))}
          </div>

          {loading && (
            <p className="text-muted-foreground flex items-center gap-1.5 text-xs">
              <Loader2 className="size-3.5 animate-spin" />
              Working out the size...
            </p>
          )}

          {error && (
            <p className="text-destructive flex items-start gap-1.5 text-xs">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              {error}
            </p>
          )}

          {plan && !loading && (
            <>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
                <dt className="text-muted-foreground">Length</dt>
                <dd className="tabular">{formatDuration(plan.durationMs)}</dd>

                <dt className="text-muted-foreground">Format</dt>
                <dd className="tabular">
                  {(plan.sampleRate / 1000).toFixed(1)} kHz,{' '}
                  {plan.channels === 1 ? 'mono' : `${plan.channels} ch`}, 16 bit
                </dd>

                <dt className="text-muted-foreground">File size</dt>
                <dd className="tabular font-medium">{formatBytes(plan.totalBytes)}</dd>
              </dl>

              {plan.mixedRates && (
                <p className="text-muted-foreground text-xs">
                  This range was recorded at more than one bit rate, so it is exported at the lowest of
                  them.
                </p>
              )}

              <p className="text-muted-foreground text-xs">
                Stretches with no recording are exported as silence, so the file lines up with the
                timeline.
              </p>

              <Button asChild className="w-full">
                {/* A plain link, so the browser streams it to disk with a progress bar rather than the
                    page holding the whole file in memory first. */}
                <a
                  href={api.exportUrl(plan.fromMs, plan.toMs)}
                  download
                  onClick={() => setOpen(false)}
                >
                  <FileAudio />
                  Download {formatBytes(plan.totalBytes)}
                </a>
              </Button>
            </>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
