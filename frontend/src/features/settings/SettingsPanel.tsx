/**
 * Audio tuning: input gain, segment length, and whether the recorder starts on its own.
 *
 * Every control here stages a change rather than applying one. Nothing reaches the server until the
 * action bar's Save is used, which is why the gain hint says "on save" rather than "immediately": the
 * server does apply gain to the live signal the instant it receives it, but it does not receive it until
 * the page is saved.
 */

import { Label } from '@/components/ui/label';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { useSettingsStore } from '@/store/useSettingsStore';

export function SettingsPanel() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading settings...</p>;
  }

  // Merged here rather than in a selector, which would hand the store a new object on every call.
  const pending = { ...settings, ...draft };

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <Label htmlFor="gain">Input gain</Label>
          <span className="text-muted-foreground text-xs tabular">{pending.gain.toFixed(2)}x</span>
        </div>
        <Slider
          id="gain"
          value={[pending.gain]}
          min={0}
          max={4}
          step={0.05}
          onValueChange={([next]) => edit({ gain: next ?? 1 })}
        />
        <p className="text-muted-foreground text-xs">
          Applied to the live signal on save. Anything above 1 can clip a hot microphone.
        </p>
      </div>

      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <Label htmlFor="segment">Segment length</Label>
          <span className="text-muted-foreground text-xs tabular">{pending.segmentSeconds} s</span>
        </div>
        <Slider
          id="segment"
          value={[pending.segmentSeconds]}
          min={5}
          max={60}
          step={5}
          onValueChange={([next]) => edit({ segmentSeconds: next ?? 10 })}
        />
        <p className="text-muted-foreground text-xs">
          Only closed segments are seekable, so a shorter segment makes recent audio scrubbable sooner.
        </p>
      </div>

      <div className="flex items-center justify-between gap-4">
        <div className="space-y-1">
          <Label htmlFor="auto-start">Record on start up</Label>
          <p className="text-muted-foreground text-xs">
            Begin capturing as soon as the service starts, without waiting for anyone to open this page.
          </p>
        </div>
        <Switch
          id="auto-start"
          checked={pending.autoStart}
          onCheckedChange={(checked) => edit({ autoStart: checked })}
        />
      </div>
    </div>
  );
}
