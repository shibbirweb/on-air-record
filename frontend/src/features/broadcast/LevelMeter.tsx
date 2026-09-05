/**
 * Input level meter for the microphone.
 *
 * Shows the signal at the recorder, not at the speakers, so it keeps moving while the listener is muted
 * or scrubbing through history. That is what makes it useful for answering "is it still recording, and is
 * the microphone actually picking anything up".
 */

import { meterScale } from '@/lib/format';
import { cn } from '@/lib/utils';

type LevelMeterProps = {
  rms: number;
  peak: number;
  active: boolean;
  className?: string;
};

export function LevelMeter({ rms, peak, active, className }: LevelMeterProps) {
  const rmsWidth = active ? meterScale(rms) * 100 : 0;
  const peakLeft = active ? meterScale(peak) * 100 : 0;
  // Anything above roughly -3 dBFS is close enough to clipping to warn about.
  const hot = peak > 0.708;

  return (
    <div className={cn('space-y-1.5', className)}>
      <div className="bg-muted relative h-2.5 w-full overflow-hidden rounded-full">
        <div
          className={cn(
            'h-full rounded-full transition-[width] duration-75 ease-out',
            hot ? 'bg-destructive' : 'bg-primary',
          )}
          style={{ width: `${rmsWidth}%` }}
        />
        {peakLeft > 0 && (
          <div
            className={cn(
              'absolute top-0 h-full w-0.5 rounded',
              hot ? 'bg-destructive' : 'bg-foreground/70',
            )}
            style={{ left: `${Math.min(peakLeft, 99.5)}%` }}
          />
        )}
      </div>
      <div className="text-muted-foreground flex justify-between text-[10px] tabular">
        <span>-60</span>
        <span>-40</span>
        <span>-20</span>
        <span className={cn(hot && 'text-destructive font-medium')}>0 dBFS</span>
      </div>
    </div>
  );
}
