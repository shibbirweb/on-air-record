/**
 * The listener count in the header, and for those allowed to see it, who those listeners are.
 *
 * Hovering opens the list and moving away closes it, with a short grace period so the pointer can travel
 * from the badge into the list. A tap or a keypress toggles it instead, since touch screens have no hover.
 * The list itself arrives over the stream socket and updates in place while open.
 */

import { Globe, UserRound } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { MouseEvent } from 'react';

import type { ListenerView } from '@/api/types';
import { Badge } from '@/components/ui/badge';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  activityTone,
  describeActivity,
  describeUserAgent,
  formatConnectedFor,
  groupListeners,
} from '@/lib/listeners';
import type { ActivityTone } from '@/lib/listeners';
import { formatDateTime } from '@/lib/format';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/store/useAuthStore';
import { useListenerCount, useListenersStore } from '@/store/useListenersStore';

/** Long enough to cross the gap between the badge and the list, short enough not to feel sticky. */
const CLOSE_DELAY_MS = 150;
/** "Connected for" is shown to the minute, so there is no point redrawing it more often than this. */
const CLOCK_TICK_MS = 15_000;

const ACTIVITY_DOT: Record<ActivityTone, string> = {
  live: 'bg-red-500',
  history: 'bg-amber-500',
  paused: 'bg-muted-foreground/50',
  // Hollow: connected, but nobody is hearing it.
  idle: 'border border-muted-foreground/60',
};

export function ListenersBadge() {
  const count = useListenerCount();
  const listeners = useListenersStore((state) => state.listeners);
  const label = `${count} ${count === 1 ? 'listener' : 'listeners'}`;

  if (listeners === null) {
    return (
      <Badge variant="outline" className="font-normal">
        {label}
      </Badge>
    );
  }
  return <ListenersMenu label={label} listeners={listeners} />;
}

function ListenersMenu({ label, listeners }: { label: string; listeners: ListenerView[] }) {
  const viewerEmail = useAuthStore((state) => state.user?.email ?? null);
  const [open, setOpen] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const closeTimer = useRef<number | null>(null);

  const cancelClose = () => {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };

  const openNow = () => {
    cancelClose();
    setNowMs(Date.now());
    setOpen(true);
  };

  const closeSoon = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };

  useEffect(() => cancelClose, []);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const timer = window.setInterval(() => setNowMs(Date.now()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, [open]);

  const onTriggerClick = (event: MouseEvent) => {
    // A mouse already opened the list by hovering, so a click must not toggle it shut again. Touch and
    // keyboard clicks carry a different pointer type and fall through to Radix's toggle.
    if ((event.nativeEvent as PointerEvent).pointerType === 'mouse') {
      event.preventDefault();
      openNow();
    }
  };

  const hoverHandlers = {
    onPointerEnter: (event: { pointerType: string }) => {
      if (event.pointerType === 'mouse') {
        openNow();
      }
    },
    onPointerLeave: (event: { pointerType: string }) => {
      if (event.pointerType === 'mouse') {
        closeSoon();
      }
    },
  };

  const groups = groupListeners(listeners, viewerEmail);

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) {
          openNow();
        } else {
          cancelClose();
          setOpen(false);
        }
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className="focus-visible:ring-ring/50 rounded-md outline-none focus-visible:ring-[3px]"
          aria-label={`${label}, show who is listening`}
          onClick={onTriggerClick}
          {...hoverHandlers}
        >
          <Badge
            variant="outline"
            className={cn('cursor-default font-normal', open && 'bg-accent text-accent-foreground')}
          >
            {label}
          </Badge>
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-80 p-0"
        // Opening on hover must not pull focus out of whatever the user was doing.
        onOpenAutoFocus={(event) => event.preventDefault()}
        {...hoverHandlers}
      >
        <div className="border-b px-3 py-2">
          <p className="text-sm font-medium">Listening now</p>
          <p className="text-muted-foreground text-xs">Updates as people connect and leave</p>
        </div>

        {groups.length === 0 ? (
          <p className="text-muted-foreground px-3 py-4 text-sm">Nobody is listening.</p>
        ) : (
          <ul className="max-h-96 divide-y overflow-y-auto">
            {groups.map((group) => (
              <li key={group.key} className="space-y-1.5 px-3 py-2.5">
                <div className="flex items-center gap-2">
                  {group.email === null ? (
                    <Globe className="text-muted-foreground size-3.5 shrink-0" />
                  ) : (
                    <UserRound className="text-muted-foreground size-3.5 shrink-0" />
                  )}
                  {/* A long address is cut to fit; the title shows it whole. */}
                  <span className="min-w-0 truncate text-sm font-medium" title={group.email ?? undefined}>
                    {group.email ?? 'Guest'}
                  </span>
                  {group.role && (
                    <Badge variant="secondary" className="px-1.5 py-0 text-[10px] font-normal">
                      {group.role === 'admin' ? 'Admin' : 'Listener'}
                    </Badge>
                  )}
                  {group.you && <span className="text-muted-foreground ml-auto shrink-0 text-xs">you</span>}
                </div>

                <ul className="space-y-1 pl-5.5">
                  {group.connections.map((connection) => (
                    <li
                      key={connection.id}
                      className="text-xs"
                      title={`${connection.userAgent ?? 'No browser description'}\nConnected ${formatDateTime(connection.connectedAtMs)}`}
                    >
                      <div className="flex items-center gap-1.5">
                        <span
                          className={cn(
                            'size-1.5 shrink-0 rounded-full',
                            ACTIVITY_DOT[activityTone(connection)],
                          )}
                        />
                        <span className="truncate">{describeActivity(connection, nowMs)}</span>
                        <span className="text-muted-foreground tabular ml-auto shrink-0">
                          {formatConnectedFor(connection.connectedAtMs, nowMs)}
                        </span>
                      </div>
                      <p className="text-muted-foreground truncate pl-3">
                        {describeUserAgent(connection.userAgent)} &middot; {connection.address}
                      </p>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        )}
      </PopoverContent>
    </Popover>
  );
}
