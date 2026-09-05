/**
 * How long recordings are kept, and what that costs in disk.
 *
 * The size projection is the point of this panel. "Keep 30 days" is meaningless until you know it means a
 * quarter of a terabyte, and the only moment that number changes anybody's mind is while they are picking
 * the window. It is computed from the bytes per hour the server reports for the format actually in use,
 * so it is exact arithmetic on raw PCM rather than a guess.
 */

import { HardDrive, Infinity as InfinityIcon, TriangleAlert } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Separator } from '@/components/ui/separator';
import { formatBytes, pcmBytesPerHour } from '@/lib/format';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';
import { useStorageStore } from '@/store/useStorageStore';

/** Familiar windows, so the common cases are one click rather than arithmetic. */
const PRESETS = [
  { label: '6 hours', hours: 6 },
  { label: '24 hours', hours: 24 },
  { label: '3 days', hours: 72 },
  { label: '7 days', hours: 168 },
  { label: '30 days', hours: 720 },
  { label: '90 days', hours: 2160 },
  { label: '1 year', hours: 8760 },
] as const;

type Unit = 'hours' | 'days';

/**
 * Show a window in whichever unit reads naturally, so 720 hours presents as 30 days.
 *
 * Days only once there is more than one of them. Exactly 24 hours is universally said as "24 hours", and
 * rendering it as "1 day" beside a highlighted "24 hours" preset looks like the two disagree.
 */
function splitWindow(hours: number): { amount: number; unit: Unit } {
  if (hours > 24 && hours % 24 === 0) {
    return { amount: hours / 24, unit: 'days' };
  }
  return { amount: hours, unit: 'hours' };
}

function toHours(amount: number, unit: Unit): number {
  return unit === 'days' ? amount * 24 : amount;
}

export function RetentionSettings() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);
  const storage = useStorageStore((state) => state.storage);
  const deviceRate = useStatusStore((state) => state.status?.capture.sampleRate ?? 0);

  /**
   * Both overrides mean "the person is driving now"; `null` means follow the stored setting. Deriving the
   * displayed values during render rather than syncing them in an effect keeps a half typed number from
   * being saved and clamped out from under the cursor, with no cascading render to arrange it.
   */
  const [amountEdit, setAmountEdit] = useState<string | null>(null);
  const [unitChoice, setUnitChoice] = useState<Unit | null>(null);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading...</p>;
  }

  // Merged in the component rather than in a selector: a selector returning a fresh object every call
  // would make the store snapshot look like it changed on every render.
  const pending = { ...settings, ...draft };
  const forever = pending.retentionHours === null;

  // Projected from the bit rate being chosen rather than the one currently recording, so picking a
  // smaller rate and a longer window shows the combined result while both are still unsaved.
  const pendingRate = pending.recordingSampleRate ?? deviceRate;
  const bytesPerHour = pendingRate > 0 ? pcmBytesPerHour(pendingRate) : (storage?.bytesPerHour ?? 0);

  const storedHours = pending.retentionHours ?? 24;
  const unit = unitChoice ?? splitWindow(storedHours).unit;
  const shownAmount =
    amountEdit ??
    String(unit === 'days' ? Math.max(Math.round(storedHours / 24), 1) : storedHours);

  const parsedAmount = Number.parseInt(shownAmount, 10);
  const draftHours =
    Number.isFinite(parsedAmount) && parsedAmount > 0 ? toHours(parsedAmount, unit) : null;

  // Project the window being edited, not the one that is saved, so the number moves with the input.
  const projectedHours = forever ? null : (draftHours ?? pending.retentionHours);
  const projectedBytes = projectedHours === null ? null : projectedHours * bytesPerHour;
  const usedBytes = storage?.bytes ?? 0;

  const applyWindow = (hours: number | null) => {
    edit({ retentionHours: hours });
  };

  const commitDraft = () => {
    if (draftHours !== null && draftHours !== pending.retentionHours) {
      applyWindow(draftHours);
    }
    setAmountEdit(null);
  };

  return (
    <div className="space-y-5">
      <RadioGroup
        value={forever ? 'forever' : 'limited'}
        onValueChange={(value) =>
          applyWindow(value === 'forever' ? null : (draftHours ?? 24))
        }
        className="gap-3"
      >
        <div className="flex items-start gap-3">
          <RadioGroupItem value="limited" id="retention-limited" className="mt-0.5" />
          <div className="grid gap-1">
            <Label htmlFor="retention-limited">Delete recordings older than</Label>
            <p className="text-muted-foreground text-xs">
              Anything past the window is removed within a minute. This is what bounds the disk.
            </p>
          </div>
        </div>

        <div className={forever ? 'pointer-events-none space-y-3 pl-7 opacity-50' : 'space-y-3 pl-7'}>
          <div className="flex items-center gap-2">
            <Input
              type="number"
              min={1}
              value={shownAmount}
              aria-label="Retention amount"
              className="w-24"
              onChange={(event) => setAmountEdit(event.target.value)}
              onBlur={commitDraft}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  commitDraft();
                }
              }}
            />
            <div className="bg-muted flex items-center gap-1 rounded-lg p-1">
              {(['hours', 'days'] as const).map((option) => (
                <Button
                  key={option}
                  size="sm"
                  variant={unit === option ? 'secondary' : 'ghost'}
                  className="h-7"
                  onClick={() => {
                    // Convert rather than reinterpret, so switching units never silently turns a window
                    // of 30 days into one of 30 hours.
                    const hours = draftHours ?? storedHours;
                    const amount = option === 'days' ? Math.max(Math.round(hours / 24), 1) : hours;
                    setUnitChoice(option);
                    setAmountEdit(null);
                    applyWindow(toHours(amount, option));
                  }}
                >
                  {option}
                </Button>
              ))}
            </div>
          </div>

          <div className="flex flex-wrap gap-1.5">
            {PRESETS.map((preset) => (
              <Button
                key={preset.hours}
                size="sm"
                variant={pending.retentionHours === preset.hours ? 'secondary' : 'outline'}
                className="h-7"
                onClick={() => {
                  setAmountEdit(null);
                  setUnitChoice(null);
                  applyWindow(preset.hours);
                }}
              >
                {preset.label}
              </Button>
            ))}
          </div>
        </div>

        <div className="flex items-start gap-3">
          <RadioGroupItem value="forever" id="retention-forever" className="mt-0.5" />
          <div className="grid gap-1">
            <Label htmlFor="retention-forever">
              <InfinityIcon className="size-3.5" />
              Keep everything forever
            </Label>
            <p className="text-muted-foreground text-xs">
              Nothing is ever deleted. The disk becomes the only limit, so watch the usage below.
            </p>
          </div>
        </div>
      </RadioGroup>

      <Separator />

      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <HardDrive className="text-muted-foreground size-4" />
          <h3 className="text-sm font-medium">Storage needed</h3>
        </div>

        <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
          <dt className="text-muted-foreground">Recording rate</dt>
          <dd className="tabular">{formatBytes(bytesPerHour)} per hour</dd>

          <dt className="text-muted-foreground">Used right now</dt>
          <dd className="tabular">{formatBytes(usedBytes)}</dd>

          <dt className="text-muted-foreground">Needed when full</dt>
          <dd className="tabular font-medium">
            {projectedBytes === null ? (
              <Badge variant="outline" className="gap-1 font-normal">
                <InfinityIcon className="size-3" />
                grows without limit
              </Badge>
            ) : (
              formatBytes(projectedBytes)
            )}
          </dd>
        </dl>

        <p className="text-muted-foreground text-xs">
          {projectedBytes === null
            ? 'With no window there is no ceiling to predict. Keep an eye on the free space of the recording disk.'
            : `Approximate, and an upper bound: the figure assumes the recorder runs without a break for the whole window at ${
                storage ? Math.round(bytesPerHour / 3_600_000) : 96
              } kB per second of uncompressed audio.`}
        </p>

        {forever && (
          <p className="text-destructive flex items-start gap-1.5 text-xs">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
            Keeping forever fills the disk in {formatBytes(bytesPerHour * 24)} per day of continuous
            recording. Nothing will stop it.
          </p>
        )}
      </div>

    </div>
  );
}
