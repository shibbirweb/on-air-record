/** Zoom levels and the follow live toggle that sit above the scrubber. */

import { LocateFixed, ZoomIn, ZoomOut } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { DayPicker } from '@/features/timeline/DayPicker';
import { formatDateTime } from '@/lib/format';
import { useTimelineStore, ZOOM_LEVELS } from '@/store/useTimelineStore';

const ZOOM_LABELS: Record<number, string> = {
  [ZOOM_LEVELS[0]]: '1m',
  [ZOOM_LEVELS[1]]: '5m',
  [ZOOM_LEVELS[2]]: '15m',
  [ZOOM_LEVELS[3]]: '1h',
  [ZOOM_LEVELS[4]]: '4h',
  [ZOOM_LEVELS[5]]: '12h',
  [ZOOM_LEVELS[6]]: '24h',
};

export function TimelineToolbar() {
  const spanMs = useTimelineStore((state) => state.spanMs);
  const followingLive = useTimelineStore((state) => state.followingLive);
  const earliestMs = useTimelineStore((state) => state.range?.earliestMs ?? null);
  const setSpan = useTimelineStore((state) => state.setSpan);
  const setFollowingLive = useTimelineStore((state) => state.setFollowingLive);

  return (
    <div className="flex flex-wrap items-center gap-2">
      <DayPicker />

      <Separator orientation="vertical" className="mx-1 h-6" />

      <Button
        size="icon-sm"
        variant="outline"
        onClick={() => setSpan(spanMs * 0.5)}
        aria-label="Zoom in"
      >
        <ZoomIn />
      </Button>
      <Button
        size="icon-sm"
        variant="outline"
        onClick={() => setSpan(spanMs * 2)}
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
            onClick={() => setSpan(level)}
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
