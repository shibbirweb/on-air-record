/**
 * The on air light.
 *
 * A studio has one because everybody in the room needs to know the state without reading anything, and the
 * same is true of a browser tab someone glances at across a desk.
 */

import { cn } from '@/lib/utils';

type OnAirSignProps = {
  live: boolean;
  className?: string;
};

export function OnAirSign({ live, className }: OnAirSignProps) {
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md border px-3 py-1.5 text-xs font-semibold tracking-widest uppercase transition-colors',
        live
          ? 'border-transparent bg-[var(--live)] text-white shadow-[0_0_18px_-2px_var(--live)]'
          : 'text-muted-foreground',
        className,
      )}
    >
      <span
        className={cn(
          'size-2 rounded-full',
          live ? 'animate-pulse bg-white' : 'bg-muted-foreground/50',
        )}
      />
      On air
    </div>
  );
}
