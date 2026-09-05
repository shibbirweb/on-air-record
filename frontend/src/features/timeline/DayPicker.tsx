/**
 * Jump the timeline to a recorded day.
 *
 * The list only offers days that actually hold audio, taken from the segment index, so there is no way to
 * pick a date and land on an empty timeline wondering whether the recorder or the picker is broken.
 *
 * Choosing a day does two things at once: it frames that day on the timeline, and it starts playback at
 * the first moment recorded in it. That combination is what makes the control usable as "play me
 * yesterday" rather than merely "scroll to yesterday", and one click on Go live undoes it.
 */

import { CalendarDays } from 'lucide-react';

import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { dayLabel, localDayId, previousDayId } from '@/lib/day';
import { formatBytes, formatClock, formatDuration } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

export function DayPicker() {
  const days = useTimelineStore((state) => state.days);
  const showDay = useTimelineStore((state) => state.showDay);
  // A primitive, so panning across midnight updates the control without re rendering on every poll.
  const activeDayId = useTimelineStore((state) => state.activeDay()?.day ?? null);
  const seek = useTransportStore((state) => state.seek);

  // The server's clock, not the browser's. The days are named in the recorder's timezone, so asking the
  // browser what "today" is would mislabel the list whenever the two machines disagree, and reading a
  // clock during render is not something a component should be doing anyway.
  const serverTimeMs = useTimelineStore((state) => state.range?.serverTimeMs ?? null);
  const todayId = serverTimeMs === null ? null : localDayId(serverTimeMs);
  const yesterdayId = serverTimeMs === null ? null : previousDayId(serverTimeMs);

  const onSelect = (dayId: string) => {
    const entry = days.find((item) => item.day === dayId);
    if (!entry) {
      return;
    }

    showDay(dayId);
    seek(entry.startMs);
  };

  if (days.length === 0) {
    return (
      <div className="text-muted-foreground flex items-center gap-2 text-xs">
        <CalendarDays className="size-3.5" />
        No recorded days yet
      </div>
    );
  }

  const activeLabel =
    activeDayId === null ? null : dayLabel(activeDayId, todayId, yesterdayId);

  return (
    <>
      <Label htmlFor="recorded-day" className="sr-only">
        Recorded day
      </Label>
      <Select value={activeDayId ?? ''} onValueChange={onSelect}>
        <SelectTrigger id="recorded-day" className="h-8 w-[150px]">
          <CalendarDays className="size-3.5 shrink-0 opacity-70" />
          {/* Own children rather than the default, which would render the whole two line item here. */}
          <SelectValue placeholder="Pick a day">{activeLabel}</SelectValue>
        </SelectTrigger>
        <SelectContent>
          {days.map((entry) => (
            <SelectItem key={entry.day} value={entry.day}>
              <span className="flex flex-col gap-0.5 py-0.5">
                <span>{dayLabel(entry.day, todayId, yesterdayId)}</span>
                <span className="text-muted-foreground text-[11px] tabular">
                  {formatClock(entry.startMs)} to {formatClock(entry.endMs)}
                  {' \u00b7 '}
                  {formatDuration(entry.recordedMs)} recorded
                  {' \u00b7 '}
                  {formatBytes(entry.bytes)}
                </span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </>
  );
}
