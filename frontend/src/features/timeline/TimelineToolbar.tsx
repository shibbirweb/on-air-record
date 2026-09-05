/** Zoom levels and the follow live toggle that sit above the scrubber. */

import { Frame, LocateFixed, ZoomIn, ZoomOut } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { BookmarkControls } from '@/features/timeline/BookmarkControls';
import { DayPicker } from '@/features/timeline/DayPicker';
import { formatDateTime } from '@/lib/format';
import { useTimelineStore, ZOOM_LEVELS } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

const ZOOM_LABELS: Record<number, string> = {
  [ZOOM_LEVELS[0]]: '1m',
  [ZOOM_LEVELS[1]]: '5m',
  [ZOOM_LEVELS[2]]: '15m',
  [ZOOM_LEVELS[3]]: '1h',
  [ZOOM_LEVELS[4]]: '4h',
  [ZOOM_LEVELS[5]]: '12h',
  [ZOOM_LEVELS[6]]: '24h',
};

type TimelineToolbarProps = {
  /** Reads the live playhead off the audio clock, same source the marker is drawn from. */
  getPlayheadMs: () => number | null;
};

export function TimelineToolbar({ getPlayheadMs }: TimelineToolbarProps) {
  const spanMs = useTimelineStore((state) => state.spanMs);
  const followingLive = useTimelineStore((state) => state.followingLive);
  const earliestMs = useTimelineStore((state) => state.range?.earliestMs ?? null);
  const zoomTo = useTimelineStore((state) => state.zoomTo);
  const resetView = useTimelineStore((state) => state.resetView);
  const setFollowingLive = useTimelineStore((state) => state.setFollowingLive);
  const requestedPositionMs = useTransportStore((state) => state.requestedPositionMs);

  /**
   * Zoom about the marker rather than the middle of the view.
   *
   * Read in the handler rather than during render, because the audio clock is a moving external value.
   * The fallback chain matches how the marker itself is drawn: the audio playhead if something is
   * playing, otherwise the moment that was cued, otherwise nothing and the window centre is used.
   */
  const zoom = (spanMs: number) => {
    zoomTo(spanMs, getPlayheadMs() ?? requestedPositionMs);
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      <DayPicker />

      <Separator orientation="vertical" className="mx-1 h-6" />

      <BookmarkControls getPlayheadMs={getPlayheadMs} />

      <Separator orientation="vertical" className="mx-1 h-6" />

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            size="icon-sm"
            variant="outline"
            onClick={() => resetView(getPlayheadMs() ?? requestedPositionMs)}
            aria-label="Reset the view"
          >
            <Frame />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Reset to the standard view</TooltipContent>
      </Tooltip>

      <Button
        size="icon-sm"
        variant="outline"
        onClick={() => zoom(spanMs * 0.5)}
        aria-label="Zoom in"
      >
        <ZoomIn />
      </Button>
      <Button
        size="icon-sm"
        variant="outline"
        onClick={() => zoom(spanMs * 2)}
        aria-label="Zoom out"
      >
        <ZoomOut />
      </Button>

      <div className="flex items-center gap-1">
        {ZOOM_LEVELS.map((level) => (
          <Button
            key={level}
            size="sm"
            variant={Math.abs(spanMs - level) < level * 0.05 ? 'secondary' : 'ghost'}
            onClick={() => zoom(level)}
          >
            {ZOOM_LABELS[level]}
          </Button>
        ))}
      </div>

      <Button
        size="sm"
        variant={followingLive ? 'secondary' : 'outline'}
        onClick={() => setFollowingLive(!followingLive)}
      >
        <LocateFixed />
        {followingLive ? 'Following live' : 'Follow live'}
      </Button>

      <Badge variant="outline" className="ml-auto font-normal">
        {earliestMs === null
          ? 'nothing recorded yet'
          : `history from ${formatDateTime(earliestMs)}`}
      </Badge>
    </div>
  );
}
