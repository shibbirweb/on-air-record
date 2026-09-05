/**
 * Runtime preferences.
 *
 * Values are committed on release rather than on every pointer move, because a slider dragged across its
 * range would otherwise fire dozens of writes, and changing the frame size restarts capture on the server.
 */

import { RotateCcw, TriangleAlert } from 'lucide-react';
import { useEffect, useState } from 'react';

import type { Settings } from '@/api/types';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';

import { Label } from '@/components/ui/label';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { useSettingsStore } from '@/store/useSettingsStore';

export function SettingsPanel() {
  const settings = useSettingsStore((state) => state.settings);
  const defaults = useSettingsStore((state) => state.defaults);
  const saving = useSettingsStore((state) => state.saving);
  const error = useSettingsStore((state) => state.error);
  const refresh = useSettingsStore((state) => state.refresh);
  const update = useSettingsStore((state) => state.update);
  const reset = useSettingsStore((state) => state.reset);

  const [confirmingReset, setConfirmingReset] = useState(false);

  /**
   * While a slider is being dragged its value lives here and overrides the stored one, so the thumb
   * tracks the pointer. On release the override is cleared and the store, updated optimistically, takes
   * over again. Mirroring the whole settings object into state instead would need an effect to keep the
   * two in step, and that effect is exactly the cascade this avoids.
   */
  const [draft, setDraft] = useState<Partial<Settings>>({});

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading settings...</p>;
  }

  const gain = draft.gain ?? settings.gain;
  const retentionHours = draft.retentionHours ?? settings.retentionHours;
  const segmentSeconds = draft.segmentSeconds ?? settings.segmentSeconds;

  // A reset that changes nothing is just a confusing button, so it is disabled when already at defaults.
  // The device is excluded because a reset deliberately leaves it alone.
  const atDefaults =
    defaults !== null &&
    settings.gain === defaults.gain &&
    settings.segmentSeconds === defaults.segmentSeconds &&
    settings.retentionHours === defaults.retentionHours &&
    settings.autoStart === defaults.autoStart &&
    settings.frameMs === defaults.frameMs;

  // Shrinking the retention window is the one genuinely destructive thing a reset can do: the janitor
  // acts within a minute and the audio is gone.
  const retentionShrinks =
    defaults !== null && defaults.retentionHours < settings.retentionHours;

  const commit = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setDraft((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    void update({ [key]: value } as Partial<Settings>);
  };

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <Label htmlFor="gain">Input gain</Label>
          <span className="text-muted-foreground text-xs tabular">{gain.toFixed(2)}x</span>
        </div>
        <Slider
          id="gain"
          value={[gain]}
          min={0}
          max={4}
          step={0.05}
          onValueChange={([next]) => setDraft((current) => ({ ...current, gain: next ?? 1 }))}
          onValueCommit={([next]) => commit('gain', next ?? 1)}
        />
        <p className="text-muted-foreground text-xs">
          Applied to the live signal immediately. Anything above 1 can clip a hot microphone.
        </p>
      </div>

      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <Label htmlFor="retention">Retention</Label>
          <span className="text-muted-foreground text-xs tabular">{retentionHours} h</span>
        </div>
        <Slider
          id="retention"
          value={[retentionHours]}
          min={1}
          max={168}
          step={1}
          onValueChange={([next]) =>
            setDraft((current) => ({ ...current, retentionHours: next ?? 24 }))
          }
          onValueCommit={([next]) => commit('retentionHours', next ?? 24)}
        />
        <p className="text-muted-foreground text-xs">
          How far back the timeline reaches. Older audio is deleted within a minute of ageing out.
        </p>
      </div>

      <div className="space-y-2">
        <div className="flex items-baseline justify-between">
          <Label htmlFor="segment">Segment length</Label>
          <span className="text-muted-foreground text-xs tabular">{segmentSeconds} s</span>
        </div>
        <Slider
          id="segment"
          value={[segmentSeconds]}
          min={5}
          max={60}
          step={5}
          onValueChange={([next]) =>
            setDraft((current) => ({ ...current, segmentSeconds: next ?? 10 }))
          }
          onValueCommit={([next]) => commit('segmentSeconds', next ?? 10)}
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
          checked={settings.autoStart}
          onCheckedChange={(checked) => void update({ autoStart: checked })}
        />
      </div>

      {saving && <p className="text-muted-foreground text-xs">Saving...</p>}
      {error && <p className="text-destructive text-xs">{error}</p>}

      <Separator />

      <div className="flex items-center justify-between gap-4">
        <p className="text-muted-foreground text-xs">
          {atDefaults
            ? 'These are the default settings.'
            : 'Some settings differ from the defaults.'}
        </p>

        <Popover open={confirmingReset} onOpenChange={setConfirmingReset}>
          <PopoverTrigger asChild>
            <Button size="sm" variant="outline" disabled={saving || atDefaults}>
              <RotateCcw />
              Reset
            </Button>
          </PopoverTrigger>

          {/* Opens upward: the button is the last thing in a long panel, so there is rarely room below. */}
          <PopoverContent side="top" align="end" className="w-72 space-y-3 p-3">
            <p className="text-sm font-medium">Restore default settings?</p>
            <p className="text-muted-foreground text-xs">
              Gain, retention, segment length, and start up behaviour go back to their defaults. The
              selected microphone is left alone.
            </p>

            {retentionShrinks && (
              <p className="text-destructive flex items-start gap-1.5 text-xs">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
                Retention drops from {settings.retentionHours} h to {defaults?.retentionHours} h. Audio
                older than that is deleted within a minute, and cannot be recovered.
              </p>
            )}

            <div className="flex justify-end gap-2 pt-1">
              <Button size="sm" variant="ghost" onClick={() => setConfirmingReset(false)}>
                Cancel
              </Button>
              <Button
                size="sm"
                variant={retentionShrinks ? 'destructive' : 'default'}
                disabled={saving}
                onClick={() => {
                  void reset();
                  setDraft({});
                  setConfirmingReset(false);
                }}
              >
                Reset
              </Button>
            </div>
          </PopoverContent>
        </Popover>
      </div>
    </div>
  );
}
