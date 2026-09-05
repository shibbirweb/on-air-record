/**
 * Downloading a span of the recording as a WAV file.
 *
 * The range opens on the window already framed on the timeline, because that is what the timeline is for
 * and asking someone to type two timestamps they have already scrolled to would be perverse. Presets
 * cover "grab what just happened", and the two fields cover everything else.
 *
 * The plan is fetched whenever the range settles, so the size, the format and any refusal are visible
 * while the range can still be adjusted, rather than arriving as a failed download.
 */

import { AlertTriangle, Download, FileAudio, Loader2 } from 'lucide-react';
import { useEffect, useState } from 'react';

import { api, ApiError } from '@/api/client';
import type { ExportPlan } from '@/api/types';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { fromDateTimeLocal, toDateTimeLocal } from '@/lib/day';
import { formatBytes, formatDateTime, formatDuration } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';

/** Quick spans measured back from the live edge, for the common "grab what just happened" case. */
const RECENT = [
  { label: 'Last 1m', ms: 60_000 },
  { label: 'Last 5m', ms: 5 * 60_000 },
  { label: 'Last 15m', ms: 15 * 60_000 },
] as const;

/** Wait for typing to settle before asking the server what the range would produce. */
const PLAN_DEBOUNCE_MS = 400;

type Range = { fromMs: number; toMs: number };

export function ExportControls() {
  const windowStartMs = useTimelineStore((state) => state.windowStartMs);
  const spanMs = useTimelineStore((state) => state.spanMs);
  const earliestMs = useTimelineStore((state) => state.range?.earliestMs ?? null);
  /**
   * The newest moment that can actually be exported, which is the end of the last coverage band rather
   * than the live edge.
   *
   * Only closed segments are indexed, so the few seconds still being written are audible live but cannot
   * be read back yet. Bounding the fields at the live edge would invite a range that always fails.
   */
  const exportableEndMs = useTimelineStore((state) => {
    const bands = state.range?.coverage;
    return bands && bands.length > 0 ? bands[bands.length - 1].endMs : null;
  });

  const [open, setOpen] = useState(false);
  const [range, setRange] = useState<Range | null>(null);
  const [plan, setPlan] = useState<ExportPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // A backwards range is a pure fact about the two fields, so it is judged during render rather than
  // stored. Only the fetch needs an effect.
  const backwards = range !== null && range.toMs <= range.fromMs;

  // Re planned whenever the range settles. Debounced because typing into a datetime field fires on every
  // keystroke, and each one would otherwise be a request.
  useEffect(() => {
    if (!open || range === null || range.toMs <= range.fromMs) {
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      setLoading(true);
      void (async () => {
        try {
          const next = await api.exportPlan(range.fromMs, range.toMs);
          if (!cancelled) {
            setPlan(next);
            setError(null);
          }
        } catch (cause) {
          if (!cancelled) {
            setPlan(null);
            setError(cause instanceof ApiError ? cause.message : 'could not work out the export');
          }
        } finally {
          if (!cancelled) {
            setLoading(false);
          }
        }
      })();
    }, PLAN_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, range]);

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (next) {
      // Snapshotted on open so the range cannot shift under the dialogue while following live scrolls
      // the timeline along.
      setRange({ fromMs: Math.round(windowStartMs), toMs: Math.round(windowStartMs + spanMs) });
      setPlan(null);
      setError(null);
    }
  };

  const edit = (field: keyof Range, value: string) => {
    const parsed = fromDateTimeLocal(value);
    if (parsed === null || range === null) {
      return;
    }
    setRange({ ...range, [field]: parsed });
  };

  // Bounds on the fields, so the native picker steers towards material that can actually be read back.
  const min = earliestMs === null ? undefined : toDateTimeLocal(earliestMs);
  const max = exportableEndMs === null ? undefined : toDateTimeLocal(exportableEndMs);

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button size="icon-sm" variant="outline" aria-label="Export audio">
          <Download />
        </Button>
      </PopoverTrigger>

      <PopoverContent align="start" className="w-84 p-0">
        <div className="space-y-1 px-3 py-2">
          <p className="text-sm font-medium">Export as WAV</p>
          <p className="text-muted-foreground text-xs">
            {exportableEndMs === null
              ? 'Nothing has been recorded yet.'
              : `Available up to ${formatDateTime(exportableEndMs)}`}
          </p>
        </div>

        <Separator />

        <div className="space-y-3 p-3">
          <div className="flex flex-wrap gap-1.5">
            <Button
              size="sm"
              variant="outline"
              className="h-7"
              onClick={() =>
                setRange({
                  fromMs: Math.round(windowStartMs),
                  toMs: Math.round(windowStartMs + spanMs),
                })
              }
            >
              Visible window
            </Button>
            {RECENT.map((option) => (
              <Button
                key={option.ms}
                size="sm"
                variant="outline"
                className="h-7"
                disabled={exportableEndMs === null}
                onClick={() => {
                  if (exportableEndMs !== null) {
                    setRange({
                      fromMs: Math.round(exportableEndMs - option.ms),
                      toMs: Math.round(exportableEndMs),
                    });
                  }
                }}
              >
                {option.label}
              </Button>
            ))}
          </div>

          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-1">
              <Label htmlFor="export-from" className="text-xs">
                From
              </Label>
              <Input
                id="export-from"
                type="datetime-local"
                step={1}
                min={min}
                max={max}
                className="h-8 text-xs"
                value={range === null ? '' : toDateTimeLocal(range.fromMs)}
                onChange={(event) => edit('fromMs', event.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="export-to" className="text-xs">
                To
              </Label>
              <Input
                id="export-to"
                type="datetime-local"
                step={1}
                min={min}
                max={max}
                className="h-8 text-xs"
                value={range === null ? '' : toDateTimeLocal(range.toMs)}
                onChange={(event) => edit('toMs', event.target.value)}
              />
            </div>
          </div>

          {loading && (
            <p className="text-muted-foreground flex items-center gap-1.5 text-xs">
              <Loader2 className="size-3.5 animate-spin" />
              Working out the size...
            </p>
          )}

          {(backwards || error) && !loading && (
            <p className="text-destructive flex items-start gap-1.5 text-xs">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              {backwards ? 'The end of the range must be after the start.' : error}
            </p>
          )}

          {plan && !loading && !backwards && (
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
                Gaps are exported as silence, so the file lines up with the timeline. The last few seconds
                of live audio cannot be exported until the segment they are in is written out.
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
