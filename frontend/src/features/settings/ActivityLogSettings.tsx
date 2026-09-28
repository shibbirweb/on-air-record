/**
 * The Activity log card: how many days entries are kept, and the way to the log.
 *
 * Staged for Save like every other setting. A number outside the range is not staged at all, rather than
 * clamped under the cursor while it is being typed; the server clamps anything that gets past this.
 */

import { useState } from 'react';
import { Link } from 'react-router';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useSettingsStore } from '@/store/useSettingsStore';

const RANGE = { min: 1, max: 3650 } as const;

const PRESETS = [
  { days: 30, label: '30 days' },
  { days: 90, label: '90 days' },
  { days: 365, label: '1 year' },
] as const;

export function ActivityLogSettings() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);
  // What is being typed, while it differs from anything stageable. `null` shows the setting itself.
  const [typed, setTyped] = useState<string | null>(null);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading...</p>;
  }

  const kept = draft.activityRetentionDays ?? settings.activityRetentionDays;
  const shown = typed ?? String(kept);
  const parsed = Number(shown);
  const valid = Number.isInteger(parsed) && parsed >= RANGE.min && parsed <= RANGE.max;

  const onChange = (text: string) => {
    const days = Number(text);
    if (text !== '' && Number.isInteger(days) && days >= RANGE.min && days <= RANGE.max) {
      edit({ activityRetentionDays: days });
      setTyped(null);
    } else {
      setTyped(text);
    }
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="activity-retention">Keep entries for</Label>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            id="activity-retention"
            aria-label="Days to keep the activity log"
            type="number"
            inputMode="numeric"
            min={RANGE.min}
            max={RANGE.max}
            className="w-24"
            value={shown}
            onChange={(event) => onChange(event.target.value)}
            onBlur={() => setTyped(null)}
          />
          <span className="text-muted-foreground text-sm">days</span>
          {PRESETS.map((preset) => (
            <Button
              key={preset.days}
              type="button"
              size="sm"
              variant={kept === preset.days && typed === null ? 'secondary' : 'outline'}
              onClick={() => {
                setTyped(null);
                edit({ activityRetentionDays: preset.days });
              }}
            >
              {preset.label}
            </Button>
          ))}
        </div>
        {!valid && (
          <p className="text-destructive text-xs">
            Between {RANGE.min} and {RANGE.max} days.
          </p>
        )}
        <p className="text-muted-foreground text-xs">
          Older entries are removed by the same clean up as old recordings, which runs every minute, but on
          their own window: keeping recordings forever does not keep the log forever.
        </p>
      </div>
      <Button asChild variant="outline" size="sm">
        <Link to="/activity">Open the activity log</Link>
      </Button>
    </div>
  );
}
