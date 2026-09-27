/**
 * What the timeline counts as a sound, for shading them and for the next and previous sound buttons.
 *
 * Offered as three words rather than a number, because the measure underneath (a multiple of each room's
 * own background) means nothing to somebody listening back, while "it misses quiet things" or "it flags the
 * fridge" is exactly what they can judge. It is part of the settings draft and waits for Save like the rest
 * of the page.
 */

import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { SoundSensitivity } from '@/api/types';
import { useSettingsStore } from '@/store/useSettingsStore';

const CHOICES: { value: SoundSensitivity; label: string; note: string }[] = [
  { value: 'low', label: 'Low', note: 'only clearly loud moments, such as a door or a raised voice' },
  { value: 'medium', label: 'Medium', note: 'ordinary speech in an ordinary room' },
  { value: 'high', label: 'High', note: 'quiet sounds too, at the cost of more of the background' },
];

export function SoundDetectionSettings() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);

  const value = draft.soundSensitivity ?? settings?.soundSensitivity ?? 'medium';
  const current = CHOICES.find((choice) => choice.value === value) ?? CHOICES[1];

  return (
    <div className="space-y-3">
      <Label htmlFor="sound-sensitivity">Sound detection</Label>

      <Select
        value={value}
        onValueChange={(next) => {
          const choice = CHOICES.find((option) => option.value === next);
          if (choice) {
            edit({ soundSensitivity: choice.value });
          }
        }}
      >
        <SelectTrigger id="sound-sensitivity" className="w-full">
          <SelectValue>{current.label}</SelectValue>
        </SelectTrigger>
        <SelectContent>
          {CHOICES.map((choice) => (
            <SelectItem key={choice.value} value={choice.value}>
              <span className="flex flex-col gap-0.5 py-0.5">
                <span>{choice.label}</span>
                <span className="text-muted-foreground text-[11px]">{choice.note}</span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <p className="text-muted-foreground text-xs">
        Decides which moments are shaded on the timeline and where the next and previous sound buttons jump.
        Each is measured against its room&apos;s own background, so a noisy room needs a louder sound than a
        quiet one. If quiet sounds are missed, choose High, or raise the input gain under Audio.
      </p>
    </div>
  );
}
