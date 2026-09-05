/**
 * Jump the timeline to a recorded day, chosen from a calendar.
 *
 * Only days that actually hold audio are selectable. The rest are visibly inert, so the calendar answers
 * "what do I have?" as well as "take me there", and there is no way to land on an empty timeline
 * wondering whether the recorder or the picker is broken. Month navigation is bounded by the oldest and
 * newest recordings for the same reason: there is nothing to find outside that range.
 *
 * Choosing a day does two things at once: it frames that day on the timeline, and it starts playback at
 * the first moment recorded in it. That is what makes the control read as "play me yesterday" rather than
 * merely "scroll to yesterday", and one click on Go live undoes it.
 */

import { CalendarDays } from 'lucide-react';
import { useMemo, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Calendar } from '@/components/ui/calendar';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { dayLabel, localDayId, parseDayId, previousDayId } from '@/lib/day';
import { formatBytes, formatClock, formatDuration } from '@/lib/format';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

export function DayPicker() {
  const [open, setOpen] = useState(false);

  const days = useTimelineStore((state) => state.days);
  const showDay = useTimelineStore((state) => state.showDay);
  // A primitive, so panning across midnight updates the control without re rendering on every poll.
  const activeDayId = useTimelineStore((state) => state.activeDay()?.day ?? null);
  // The server's clock, not the browser's. Days are named in the recorder's timezone, so asking the
  // browser what "today" is would mislabel the button whenever the two machines disagree.
  const serverTimeMs = useTimelineStore((state) => state.range?.serverTimeMs ?? null);
  const seek = useTransportStore((state) => state.seek);

  const recorded = useMemo(() => {
    const byId = new Map(days.map((entry) => [entry.day, entry]));
    const dates = days
      .map((entry) => parseDayId(entry.day))
      .filter((date): date is Date => date !== null)
      .sort((left, right) => left.getTime() - right.getTime());

    return { byId, earliest: dates.at(0), latest: dates.at(-1) };
  }, [days]);

  const todayId = serverTimeMs === null ? null : localDayId(serverTimeMs);
  const yesterdayId = serverTimeMs === null ? null : previousDayId(serverTimeMs);

  const activeEntry = activeDayId === null ? undefined : recorded.byId.get(activeDayId);
  const activeLabel =
    activeDayId === null ? 'Pick a day' : dayLabel(activeDayId, todayId, yesterdayId);
  // The window can sit on a day with no recording, in which case nothing is highlighted.
  const selectedDate = activeEntry ? (parseDayId(activeEntry.day) ?? undefined) : undefined;

  const onSelect = (date: Date | undefined) => {
    if (!date) {
      return;
    }

    const entry = recorded.byId.get(localDayId(date.getTime()));
    if (!entry) {
      return;
    }

    showDay(entry.day);
    seek(entry.startMs);
    setOpen(false);
  };

  if (days.length === 0) {
    return (
      <div className="text-muted-foreground flex items-center gap-2 text-xs">
        <CalendarDays className="size-3.5" />
        No recorded days yet
      </div>
    );
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="h-8 w-37.5 justify-start font-normal"
          aria-label="Choose a recorded day"
        >
          <CalendarDays className="size-3.5 shrink-0 opacity-70" />
          <span className="truncate">{activeLabel}</span>
        </Button>
      </PopoverTrigger>

      <PopoverContent className="p-0">
        <Calendar
          className="p-3"
          mode="single"
          // Put keyboard focus on the grid rather than the month arrows, so the calendar is arrow key
          // navigable the moment it opens instead of after a tab or two.
          autoFocus
          selected={selectedDate}
          defaultMonth={selectedDate ?? recorded.latest}
          startMonth={recorded.earliest}
          endMonth={recorded.latest}
          // The matcher is the whole feature: a day the recorder never wrote to is not a destination.
          disabled={(date) => !recorded.byId.has(localDayId(date.getTime()))}
          onSelect={onSelect}
        />

        <Separator />

        <div className="text-muted-foreground px-3 py-2 text-xs">
          {activeEntry ? (
            <span className="tabular">
              {formatClock(activeEntry.startMs)} to {formatClock(activeEntry.endMs)}
              {' '}
              &middot;{' '}
              {formatDuration(activeEntry.recordedMs)} recorded
              {' '}
              &middot;{' '}
              {formatBytes(activeEntry.bytes)}
            </span>
          ) : (
            <span>
              {days.length} {days.length === 1 ? 'day holds' : 'days hold'} recordings. Days without
              audio cannot be picked.
            </span>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
